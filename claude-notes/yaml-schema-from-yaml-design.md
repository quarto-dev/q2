# YAML Schema Deserialization Design

## Executive Summary

Design for loading Quarto YAML schemas from YAML files (as used in quarto-cli) into Rust `Schema` enum for validation. This enables the `validate-yaml` binary and future schema-driven validation.

**Key Architectural Decision**: Use `YamlWithSourceInfo` for schema loading (not serde deserialization) to ensure YAML 1.2 compatibility and source tracking.

### Why This Matters

1. **YAML 1.2 Requirement**: User documents use YAML 1.2 (via yaml-rust2). Schemas must use the same to avoid inconsistent parsing behavior (e.g., `no` as string vs boolean).

2. **Extensions Support**: Quarto extensions will declare their own schemas using the same infrastructure. They need YAML 1.2 consistency too.

3. **Source Tracking**: Schema validation errors should point to exact locations in schema files. YamlWithSourceInfo provides this out of the box.

4. **Previous Approach**: The current implementation uses serde deserialization (YAML 1.1 via serde_yaml). This was acceptable for prototyping but must be replaced.

### Architectural Change Summary

| Aspect | Old (serde) | New (YamlWithSourceInfo) |
|--------|-------------|---------------------------|
| Parsing | `impl Deserialize for Schema` | `impl Schema { fn from_yaml() }` |
| YAML Version | 1.1 (yaml-rust) | 1.2 (yaml-rust2) |
| Source Tracking | No | Yes (SourceInfo for every element) |
| Error Messages | Generic serde errors | Custom errors with locations |
| Extensibility | Limited by serde | Full control |
| Dependency | serde_yaml | quarto-yaml |

## Current State

**Implemented**: `quarto-yaml-validation` crate with:
- `Schema` enum (12 variants: False, True, Boolean, Number, String, Null, Enum, Any, AnyOf, AllOf, Array, Object, Ref)
- Validation against programmatic schemas
- ~920 LOC in schema.rs, Phase 1 complete
- **TEMPORARY**: Uses serde deserialization (YAML 1.1) - must be replaced

**Missing**: Ability to load schemas from YAML files using YAML 1.2

## Critical Requirements

### YAML 1.2 Compatibility

**We CANNOT use `serde_yaml` because it only supports YAML 1.1.**

See `/crates/quarto-yaml/YAML-1.2-REQUIREMENT.md` and `/crates/quarto-yaml-validation/YAML-1.2-REQUIREMENT.md` for details.

**Impact**: Schema loading must use `YamlWithSourceInfo` (which uses yaml-rust2 with YAML 1.2 support) instead of serde deserialization.

### Quarto Extensions Support

**Design goal**: Future Quarto extensions must be able to declare their own schemas using exactly the same infrastructure as core Quarto.

This means:
- Extensions define schemas in YAML files
- Extensions use `quarto-yaml-validation` to validate documents
- Everything uses YAML 1.2 consistently
- Schema validation errors include source locations

## quarto-cli Schema Format Analysis

### File Structure

Schema files in quarto-cli contain **arrays of schema definitions**:

```yaml
# Example: document-execute.yml
- name: engine
  schema:
    string:
      completions: [jupyter, knitr, julia]
  description: "Engine used for executable code blocks."

- name: cache
  schema:
    anyOf:
      - boolean
      - enum: [refresh]
  default: false
  description:
    short: "Cache results of computations."
    long: |
      Detailed explanation...
```

**Key insight**: Each file is an array of field definitions, not a single schema.

### Schema Definition Syntax

Two syntaxes coexist:

#### 1. Explicit `schema:` Field (Most Common)

Used when additional metadata (description, default, etc.) is needed:

```yaml
- name: cache
  schema:
    anyOf:
      - boolean
      - enum: [refresh]
  default: false
  description: "..."
```

#### 2. Inline Schema (Simpler)

Used in `definitions.yml` and `schema.yml` for reusable schemas:

```yaml
- id: math-methods
  enum:
    values: [plain, webtex, gladtex, mathml, mathjax, katex]
```

No `schema:` wrapper, just schema directly with an `id:` field.

### Schema Type Syntaxes

All from analyzing `schema.yml` (the meta-schema):

#### Boolean
```yaml
boolean              # Short form
boolean:             # Long form with metadata
  description: "..."
```

#### Number
```yaml
number               # Short form
number:              # Long form
  description: "..."
```

#### String
```yaml
string               # Short form
string:              # Long form
  pattern: "regex"
  completions: ["a", "b"]
path                 # Alias for string (file paths)
```

#### Null
```yaml
null                 # Short form
"null":              # Long form (key must be quoted!)
  description: "..."
```

#### Enum
```yaml
enum: [val1, val2, val3]              # Short form
enum:                                  # Long form
  values: [val1, val2, val3]
  description: "..."
```

#### Any
```yaml
any                  # Accepts any value
```

#### AnyOf
```yaml
anyOf:               # Array form (most common)
  - boolean
  - string
  - enum: [null]

anyOf:               # Object form (with metadata)
  schemas:
    - boolean
    - string
```

#### AllOf
```yaml
allOf:               # Array form
  - string
  - object:
      properties:
        foo: string

allOf:               # Object form
  schemas:
    - string
    - object: { ... }
```

#### Array
```yaml
arrayOf: string                        # Simple array
arrayOf:                               # Complex array
  schema: string
  length: 5                            # Fixed length
```

#### MaybeArrayOf (Quarto Extension)
```yaml
maybeArrayOf: string   # Accepts string OR array of string
```

#### Object
```yaml
object                                 # Any object
object:
  properties:
    foo: string
    bar: number
  required: [foo]                      # Or "all"
  closed: true                         # No additional properties
  super:                               # Inheritance
    resolveRef: schema/base
  additionalProperties: string         # Type for extra props
  patternProperties:                   # Regex-matched props
    "^x-": string
```

#### Record (Quarto-specific)
```yaml
record:                                # Object with uniform value type
  properties:
    "key1": string
    "key2": string
    # All properties have same type

record:                                # Explicit form
  properties:
    properties:
      "key1": string
```

**Difference from Object**: Record enforces all values have same type, used for dictionaries/maps.

#### Ref (Reference to Named Schema)
```yaml
ref: schema/base       # Reference by ID
ref: date              # Reference by name
```

Two types of references:
- `ref: <id>` - Normal reference (looks up schema by ID)
- `resolveRef: <id>` - Used in `super:` for inheritance

### Base Schema Properties

All schemas can have these optional properties (from `schema/base`):

```yaml
id: unique-identifier
description:
  short: "One line"
  long: |
    Multiple
    lines
hidden: true                    # Don't show in completions
tags:
  execute-only: true
errorDescription: "Custom error message"
default: value
completions: ["a", "b"]         # Completion suggestions
additionalCompletions: ["c"]
```

### Meta-Schema

The file `schema.yml` defines schemas for schemas:

```yaml
- id: schema/schema
  anyOf:
    - ref: schema/enum
    - ref: schema/null
    - ref: schema/explicit-schema
    - ref: schema/string
    - ref: schema/number
    - ref: schema/boolean
    - ref: schema/ref
    - ref: schema/resolve-ref
    - ref: schema/any-of
    - ref: schema/array-of
    - ref: schema/maybe-array-of
    - ref: schema/all-of
    - ref: schema/record
    - ref: schema/object
    - enum: [null, "any"]
  description: "be a yaml schema"
```

This is the recursive definition: a schema is one of these types.

## Design for Rust Implementation

### Architecture: YamlWithSourceInfo-Based Parsing

**Replace serde deserialization with manual parsing from `YamlWithSourceInfo`.**

This approach:
1. ✅ Ensures YAML 1.2 compatibility (via yaml-rust2)
2. ✅ Preserves source location information for error messages
3. ✅ Provides same infrastructure for Quarto extensions
4. ✅ Gives full control over validation and error messages

#### Challenge: Multiple Syntaxes

The quarto-cli schema format has multiple syntaxes:
```yaml
boolean              # String "boolean"
boolean:             # Object with properties
  description: "..."
```

**Solution**: Pattern match on `YamlWithSourceInfo` structure and convert to `Schema` manually.

### Phase 1: Schema Parsing from YamlWithSourceInfo

Add parsing methods to existing `Schema` enum in `quarto-yaml-validation`.

#### Implementation Strategy

```rust
// In quarto-yaml-validation/src/schema.rs

use quarto_yaml::{YamlWithSourceInfo, SourceInfo};
use yaml_rust2::Yaml;

/// Error type for schema parsing
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("Invalid schema type: {0}")]
    InvalidType(String),

    #[error("Invalid schema structure at {location}: {message}")]
    InvalidStructure {
        message: String,
        location: SourceInfo,
    },

    #[error("Missing required field '{field}' at {location}")]
    MissingField {
        field: String,
        location: SourceInfo,
    },

    #[error("Unresolved schema reference: {0}")]
    UnresolvedRef(String),
}

impl Schema {
    /// Parse a Schema from YamlWithSourceInfo.
    ///
    /// This supports all quarto-cli schema syntaxes:
    /// - Short forms: "boolean", "string", "number", etc.
    /// - Object forms: {boolean: {...}}, {string: {...}}, etc.
    /// - Inline arrays: [val1, val2, val3] (for enums)
    pub fn from_yaml(yaml: &YamlWithSourceInfo) -> Result<Schema, SchemaError> {
        match &yaml.yaml {
            // Short form: "boolean", "string", etc.
            Yaml::String(s) => Self::parse_short_form(s.as_str(), &yaml.source_info),

            // Object form: {boolean: {...}}, {enum: [...]}, etc.
            Yaml::Hash(_) => Self::parse_object_form(yaml),

            // Array form: [val1, val2, val3] - inline enum
            Yaml::Array(_) => Self::parse_inline_enum(yaml),

            // Null can be a schema type too
            Yaml::Null => Ok(Schema::Null(NullSchema {
                annotations: Default::default(),
            })),

            _ => Err(SchemaError::InvalidStructure {
                message: format!("Expected schema, got {:?}", yaml.yaml),
                location: yaml.source_info.clone(),
            }),
        }
    }

    /// Parse short form: "boolean", "string", "number", "any", "null", "path"
    fn parse_short_form(s: &str, location: &SourceInfo) -> Result<Schema, SchemaError> {
        match s {
            "boolean" => Ok(Schema::Boolean(BooleanSchema {
                annotations: Default::default(),
            })),
            "number" => Ok(Schema::Number(NumberSchema {
                annotations: Default::default(),
                minimum: None,
                maximum: None,
                exclusive_minimum: None,
                exclusive_maximum: None,
                multiple_of: None,
            })),
            "string" | "path" => Ok(Schema::String(StringSchema {
                annotations: Default::default(),
                min_length: None,
                max_length: None,
                pattern: None,
            })),
            "null" => Ok(Schema::Null(NullSchema {
                annotations: Default::default(),
            })),
            "any" => Ok(Schema::Any(AnySchema {
                annotations: Default::default(),
            })),
            _ => Err(SchemaError::InvalidType(s.to_string())),
        }
    }

    /// Parse object form: {boolean: {...}}, {string: {...}}, etc.
    fn parse_object_form(yaml: &YamlWithSourceInfo) -> Result<Schema, SchemaError> {
        let entries = yaml.as_hash().ok_or_else(|| SchemaError::InvalidStructure {
            message: "Expected hash for object form schema".to_string(),
            location: yaml.source_info.clone(),
        })?;

        if entries.is_empty() {
            return Err(SchemaError::InvalidStructure {
                message: "Empty schema object".to_string(),
                location: yaml.source_info.clone(),
            });
        }

        // Peek at first key to determine schema type
        let first_entry = &entries[0];
        let key = first_entry.key.yaml.as_str().ok_or_else(|| {
            SchemaError::InvalidStructure {
                message: "Schema type key must be a string".to_string(),
                location: first_entry.key.source_info.clone(),
            }
        })?;

        match key {
            "boolean" => Self::parse_boolean_schema(&first_entry.value),
            "number" => Self::parse_number_schema(&first_entry.value),
            "string" | "path" => Self::parse_string_schema(&first_entry.value),
            "null" => Self::parse_null_schema(&first_entry.value),
            "enum" => Self::parse_enum_schema(&first_entry.value),
            "any" => Self::parse_any_schema(&first_entry.value),
            "anyOf" => Self::parse_anyof_schema(&first_entry.value),
            "allOf" => Self::parse_allof_schema(&first_entry.value),
            "array" => Self::parse_array_schema(&first_entry.value),
            "object" => Self::parse_object_schema(&first_entry.value),
            "ref" | "$ref" => Self::parse_ref_schema(&first_entry.value),
            _ => Err(SchemaError::InvalidType(key.to_string())),
        }
    }

    /// Parse inline enum array: [val1, val2, val3]
    fn parse_inline_enum(yaml: &YamlWithSourceInfo) -> Result<Schema, SchemaError> {
        let items = yaml.as_array().ok_or_else(|| SchemaError::InvalidStructure {
            message: "Expected array for inline enum".to_string(),
            location: yaml.source_info.clone(),
        })?;

        // Convert YamlWithSourceInfo items to serde_json::Value for enum values
        let values: Result<Vec<_>, _> = items
            .iter()
            .map(|item| yaml_to_json_value(&item.yaml))
            .collect();

        Ok(Schema::Enum(EnumSchema {
            annotations: Default::default(),
            values: values?,
        }))
    }

    // Individual type parsers...

    fn parse_boolean_schema(yaml: &YamlWithSourceInfo) -> Result<Schema, SchemaError> {
        let annotations = Self::parse_annotations(yaml)?;
        Ok(Schema::Boolean(BooleanSchema { annotations }))
    }

    fn parse_number_schema(yaml: &YamlWithSourceInfo) -> Result<Schema, SchemaError> {
        let annotations = Self::parse_annotations(yaml)?;

        // Extract number-specific fields
        let minimum = Self::get_hash_number(yaml, "minimum")?;
        let maximum = Self::get_hash_number(yaml, "maximum")?;
        let exclusive_minimum = Self::get_hash_number(yaml, "exclusiveMinimum")?;
        let exclusive_maximum = Self::get_hash_number(yaml, "exclusiveMaximum")?;
        let multiple_of = Self::get_hash_number(yaml, "multipleOf")?;

        Ok(Schema::Number(NumberSchema {
            annotations,
            minimum,
            maximum,
            exclusive_minimum,
            exclusive_maximum,
            multiple_of,
        }))
    }

    // ... more type parsers (string, enum, anyOf, object, etc.)

    /// Parse common annotations from a schema object
    fn parse_annotations(yaml: &YamlWithSourceInfo) -> Result<SchemaAnnotations, SchemaError> {
        let hash = match yaml.as_hash() {
            Some(h) => h,
            None => return Ok(Default::default()),
        };

        Ok(SchemaAnnotations {
            id: Self::get_hash_string(yaml, "$id")?,
            description: Self::get_hash_string(yaml, "description")?,
            documentation: Self::get_hash_string(yaml, "documentation")?,
            error_message: Self::get_hash_string(yaml, "errorMessage")?,
            hidden: Self::get_hash_bool(yaml, "hidden")?,
            completions: Self::get_hash_string_array(yaml, "completions")?,
            tags: Self::get_hash_tags(yaml)?,
        })
    }

    // Helper methods for extracting typed values from hashes

    fn get_hash_string(yaml: &YamlWithSourceInfo, key: &str) -> Result<Option<String>, SchemaError> {
        if let Some(value) = yaml.get_hash_value(key) {
            if let Some(s) = value.yaml.as_str() {
                return Ok(Some(s.to_string()));
            }
            return Err(SchemaError::InvalidStructure {
                message: format!("Field '{}' must be a string", key),
                location: value.source_info.clone(),
            });
        }
        Ok(None)
    }

    fn get_hash_number(yaml: &YamlWithSourceInfo, key: &str) -> Result<Option<f64>, SchemaError> {
        if let Some(value) = yaml.get_hash_value(key) {
            match &value.yaml {
                Yaml::Integer(i) => return Ok(Some(*i as f64)),
                Yaml::Real(r) => {
                    if let Ok(f) = r.parse::<f64>() {
                        return Ok(Some(f));
                    }
                }
                _ => {}
            }
            return Err(SchemaError::InvalidStructure {
                message: format!("Field '{}' must be a number", key),
                location: value.source_info.clone(),
            });
        }
        Ok(None)
    }

    // ... more helpers
}

/// Convert yaml-rust2 Yaml to serde_json::Value (for enum values)
fn yaml_to_json_value(yaml: &Yaml) -> Result<serde_json::Value, SchemaError> {
    match yaml {
        Yaml::String(s) => Ok(serde_json::Value::String(s.clone())),
        Yaml::Integer(i) => Ok(serde_json::Value::Number((*i).into())),
        Yaml::Real(r) => {
            if let Ok(f) = r.parse::<f64>() {
                if let Some(n) = serde_json::Number::from_f64(f) {
                    return Ok(serde_json::Value::Number(n));
                }
            }
            Err(SchemaError::InvalidStructure {
                message: format!("Invalid number: {}", r),
                location: SourceInfo::default(), // TODO: pass location
            })
        }
        Yaml::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
        Yaml::Null => Ok(serde_json::Value::Null),
        _ => Err(SchemaError::InvalidStructure {
            message: "Unsupported YAML type for enum value".to_string(),
            location: SourceInfo::default(),
        }),
    }
}
```

### Phase 2: Schema Field Definitions

Schema files contain arrays of field definitions. Create wrapper type that parses from `YamlWithSourceInfo`:

```rust
// In quarto-yaml-validation/src/schema_file.rs (new file)

use quarto_yaml::{YamlWithSourceInfo, parse_file};
use crate::schema::{Schema, SchemaError};
use std::path::Path;

/// A field definition in a schema file.
///
/// Quarto schema files contain arrays of these field definitions, each with
/// optional metadata (name, id, description) and a schema.
#[derive(Debug, Clone)]
pub struct SchemaField {
    /// Field name (for field-based schemas)
    pub name: Option<String>,

    /// Schema ID (for reusable schemas in definitions.yml)
    pub id: Option<String>,

    /// The schema itself
    pub schema: Schema,

    /// Description (short or long form)
    pub description: Option<Description>,

    /// Default value (as YAML)
    pub default: Option<YamlWithSourceInfo>,

    /// Hidden from completions
    pub hidden: bool,

    /// Tags (metadata as key-value pairs)
    pub tags: HashMap<String, YamlWithSourceInfo>,

    /// Error description override
    pub error_description: Option<String>,

    /// Source location of this field definition
    pub source_info: SourceInfo,
}

/// Description can be short string or long form with short+long
#[derive(Debug, Clone)]
pub enum Description {
    Short(String),
    Long {
        short: String,
        long: String,
    },
}

impl SchemaField {
    /// Parse a SchemaField from YamlWithSourceInfo.
    ///
    /// Handles both syntaxes:
    /// 1. Explicit schema: field (has `schema:` field)
    /// 2. Inline schema (everything is the schema)
    pub fn from_yaml(yaml: &YamlWithSourceInfo) -> Result<SchemaField, SchemaError> {
        let hash = yaml.as_hash().ok_or_else(|| SchemaError::InvalidStructure {
            message: "Schema field must be an object".to_string(),
            location: yaml.source_info.clone(),
        })?;

        // Extract metadata fields
        let name = get_hash_string(yaml, "name")?;
        let id = get_hash_string(yaml, "id")?;
        let hidden = get_hash_bool(yaml, "hidden")?.unwrap_or(false);
        let error_description = get_hash_string(yaml, "errorDescription")?;

        // Parse description (can be string or object)
        let description = if let Some(desc_yaml) = yaml.get_hash_value("description") {
            Some(Description::from_yaml(desc_yaml)?)
        } else {
            None
        };

        // Parse default value (keep as YamlWithSourceInfo)
        let default = yaml.get_hash_value("default").cloned();

        // Parse tags (keep as YamlWithSourceInfo for now)
        let tags = if let Some(tags_yaml) = yaml.get_hash_value("tags") {
            Self::parse_tags(tags_yaml)?
        } else {
            HashMap::new()
        };

        // Parse schema: explicit `schema:` field OR inline
        let schema = if let Some(schema_yaml) = yaml.get_hash_value("schema") {
            // Explicit schema field
            Schema::from_yaml(schema_yaml)?
        } else {
            // Inline schema - need to filter out metadata fields
            // and parse the rest as a schema
            Self::parse_inline_schema(yaml)?
        };

        Ok(SchemaField {
            name,
            id,
            schema,
            description,
            default,
            hidden,
            tags,
            error_description,
            source_info: yaml.source_info.clone(),
        })
    }

    /// Parse inline schema by filtering out metadata fields
    fn parse_inline_schema(yaml: &YamlWithSourceInfo) -> Result<Schema, SchemaError> {
        // Metadata fields that should be filtered out
        const METADATA_FIELDS: &[&str] = &[
            "name", "id", "description", "default", "hidden",
            "tags", "errorDescription", "completions"
        ];

        let hash = yaml.as_hash().ok_or_else(|| SchemaError::InvalidStructure {
            message: "Expected hash for inline schema".to_string(),
            location: yaml.source_info.clone(),
        })?;

        // Find the schema key (should be the first non-metadata key)
        let schema_entry = hash.iter().find(|entry| {
            if let Some(key_str) = entry.key.yaml.as_str() {
                !METADATA_FIELDS.contains(&key_str)
            } else {
                false
            }
        }).ok_or_else(|| SchemaError::InvalidStructure {
            message: "No schema found in field definition".to_string(),
            location: yaml.source_info.clone(),
        })?;

        // Parse just the schema part
        let schema_type = schema_entry.key.yaml.as_str().unwrap();

        // Reconstruct schema YAML with just this one key-value pair
        // This is a bit tricky - we need to create a new YamlWithSourceInfo
        // that looks like {<schema_type>: <value>}
        Schema::from_yaml(yaml)  // TODO: Need to construct proper schema YAML
    }

    fn parse_tags(yaml: &YamlWithSourceInfo) -> Result<HashMap<String, YamlWithSourceInfo>, SchemaError> {
        let hash = yaml.as_hash().ok_or_else(|| SchemaError::InvalidStructure {
            message: "Tags must be an object".to_string(),
            location: yaml.source_info.clone(),
        })?;

        let mut tags = HashMap::new();
        for entry in hash {
            if let Some(key) = entry.key.yaml.as_str() {
                tags.insert(key.to_string(), entry.value.clone());
            }
        }
        Ok(tags)
    }
}

impl Description {
    fn from_yaml(yaml: &YamlWithSourceInfo) -> Result<Description, SchemaError> {
        // String form: just "description text"
        if let Some(s) = yaml.yaml.as_str() {
            return Ok(Description::Short(s.to_string()));
        }

        // Object form: {short: "...", long: "..."}
        if let Some(_hash) = yaml.as_hash() {
            let short = get_hash_string(yaml, "short")?
                .ok_or_else(|| SchemaError::MissingField {
                    field: "short".to_string(),
                    location: yaml.source_info.clone(),
                })?;

            let long = get_hash_string(yaml, "long")?
                .ok_or_else(|| SchemaError::MissingField {
                    field: "long".to_string(),
                    location: yaml.source_info.clone(),
                })?;

            return Ok(Description::Long { short, long });
        }

        Err(SchemaError::InvalidStructure {
            message: "Description must be string or object with short/long".to_string(),
            location: yaml.source_info.clone(),
        })
    }
}

/// Load schema file (array of field definitions)
pub fn load_schema_file(path: &Path) -> Result<Vec<SchemaField>, SchemaError> {
    let yaml = parse_file(path).map_err(|e| SchemaError::InvalidStructure {
        message: format!("Failed to parse YAML: {}", e),
        location: SourceInfo::default(),
    })?;

    // Schema files are arrays of field definitions
    let array = yaml.as_array().ok_or_else(|| SchemaError::InvalidStructure {
        message: "Schema file must contain an array of field definitions".to_string(),
        location: yaml.source_info.clone(),
    })?;

    array.iter()
        .map(|item| SchemaField::from_yaml(item))
        .collect()
}

// Helper functions (similar to Schema impl)

fn get_hash_string(yaml: &YamlWithSourceInfo, key: &str) -> Result<Option<String>, SchemaError> {
    if let Some(value) = yaml.get_hash_value(key) {
        if let Some(s) = value.yaml.as_str() {
            return Ok(Some(s.to_string()));
        }
        return Err(SchemaError::InvalidStructure {
            message: format!("Field '{}' must be a string", key),
            location: value.source_info.clone(),
        });
    }
    Ok(None)
}

fn get_hash_bool(yaml: &YamlWithSourceInfo, key: &str) -> Result<Option<bool>, SchemaError> {
    if let Some(value) = yaml.get_hash_value(key) {
        if let Some(b) = value.yaml.as_bool() {
            return Ok(Some(b));
        }
        return Err(SchemaError::InvalidStructure {
            message: format!("Field '{}' must be a boolean", key),
            location: value.source_info.clone(),
        });
    }
    Ok(None)
}
```

### Phase 3: Schema Registry

Support `ref:` lookups:

```rust
/// Registry of named schemas for resolving refs
pub struct SchemaRegistry {
    schemas: HashMap<String, Schema>,
}

impl SchemaRegistry {
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
        }
    }

    /// Load schemas from a file and register them
    pub fn load_file(&mut self, path: &Path) -> Result<(), Error> {
        let fields = load_schema_file(path)?;
        for field in fields {
            if let Some(id) = field.id {
                self.register(id, field.schema);
            } else if let Some(name) = field.name {
                self.register(name, field.schema);
            }
        }
        Ok(())
    }

    /// Load all schemas from a directory
    pub fn load_directory(&mut self, dir: &Path) -> Result<(), Error> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension() == Some("yml") || path.extension() == Some("yaml") {
                self.load_file(&path)?;
            }
        }
        Ok(())
    }

    pub fn register(&mut self, id: impl Into<String>, schema: Schema) {
        self.schemas.insert(id.into(), schema);
    }

    pub fn resolve(&self, ref_id: &str) -> Option<&Schema> {
        self.schemas.get(ref_id)
    }

    /// Resolve all refs in a schema recursively
    pub fn resolve_refs(&self, schema: &Schema) -> Result<Schema, Error> {
        match schema {
            Schema::Ref(id) => {
                let resolved = self.resolve(id)
                    .ok_or_else(|| Error::UnresolvedRef(id.clone()))?;
                // Recursively resolve in case the ref points to another ref
                self.resolve_refs(resolved)
            }
            Schema::AnyOf(schemas) => {
                let resolved: Result<Vec<_>, _> = schemas
                    .iter()
                    .map(|s| self.resolve_refs(s))
                    .collect();
                Ok(Schema::AnyOf(resolved?))
            }
            Schema::AllOf(schemas) => {
                // Similar to AnyOf
                // ...
            }
            Schema::Array(inner) => {
                Ok(Schema::Array(Box::new(self.resolve_refs(inner)?)))
            }
            Schema::Object(fields, opts) => {
                let resolved_fields: Result<HashMap<_, _>, _> = fields
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), self.resolve_refs(v)?)))
                    .collect();
                Ok(Schema::Object(resolved_fields?, opts.clone()))
            }
            // Other variants that don't contain schemas
            _ => Ok(schema.clone()),
        }
    }
}
```

### Phase 4: Integration with Validation

Current validation API:

```rust
pub fn validate(
    value: &YamlWithSourceInfo,
    schema: &Schema,
    ctx: &mut ValidationContext,
) -> Result<(), ()>
```

With registry:

```rust
pub fn validate_with_registry(
    value: &YamlWithSourceInfo,
    schema: &Schema,
    registry: &SchemaRegistry,
    ctx: &mut ValidationContext,
) -> Result<(), ()> {
    // Resolve refs on-the-fly during validation
    let schema = match schema {
        Schema::Ref(id) => {
            registry.resolve(id)
                .ok_or_else(|| {
                    ctx.add_error(ValidationError::unresolved_ref(id, value.source_info()));
                })?
        }
        _ => schema,
    };

    // Continue with normal validation
    validate(value, schema, ctx)
}
```

## validate-yaml Binary Design

### CLI Interface

```bash
validate-yaml <yaml-file> <schema-file>
```

**Arguments**:
- `<yaml-file>`: Path to YAML file to validate (contains a single YAML object)
- `<schema-file>`: Path to schema file (contains array of schema definitions)

**Behavior**:
1. Load schema file
2. If multiple schemas in file, use the first one (or require `--schema-id` flag)
3. Parse YAML file
4. Validate
5. Output results (pretty errors via quarto-error-reporting)

### Binary Crate Structure

```
crates/validate-yaml/
├── Cargo.toml
└── src/
    └── main.rs
```

**Cargo.toml**:
```toml
[package]
name = "validate-yaml"
version.workspace = true
edition.workspace = true

[[bin]]
name = "validate-yaml"
path = "src/main.rs"

[dependencies]
quarto-yaml = { workspace = true }
quarto-yaml-validation = { workspace = true }
quarto-error-reporting = { workspace = true }
clap = { workspace = true }
anyhow = { workspace = true }
```

**main.rs**:
```rust
use clap::Parser;
use quarto_yaml::parse;
use quarto_yaml_validation::{load_schema_file, validate_with_registry, SchemaRegistry, ValidationContext};
use quarto_error_reporting::DiagnosticMessageBuilder;
use std::path::PathBuf;
use std::process;

#[derive(Parser)]
#[command(name = "validate-yaml")]
#[command(about = "Validate a YAML file against a schema")]
struct Cli {
    /// Path to YAML file to validate
    yaml_file: PathBuf,

    /// Path to schema file (YAML)
    schema_file: PathBuf,

    /// Schema ID to use (if schema file contains multiple schemas)
    #[arg(long)]
    schema_id: Option<String>,

    /// Additional schema directories for resolving refs
    #[arg(long = "schema-dir")]
    schema_dirs: Vec<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    // Load schema
    let schema_fields = load_schema_file(&cli.schema_file)
        .unwrap_or_else(|e| {
            eprintln!("Error loading schema: {}", e);
            process::exit(1);
        });

    let schema = if let Some(id) = &cli.schema_id {
        schema_fields
            .iter()
            .find(|f| f.id.as_ref() == Some(id) || f.name.as_ref() == Some(id))
            .map(|f| &f.schema)
            .unwrap_or_else(|| {
                eprintln!("Schema '{}' not found in {}", id, cli.schema_file.display());
                process::exit(1);
            })
    } else if schema_fields.len() == 1 {
        &schema_fields[0].schema
    } else {
        eprintln!("Schema file contains {} schemas. Use --schema-id to specify which one.", schema_fields.len());
        process::exit(1);
    };

    // Build schema registry
    let mut registry = SchemaRegistry::new();

    // Load additional schema directories
    for dir in &cli.schema_dirs {
        if let Err(e) = registry.load_directory(dir) {
            eprintln!("Warning: Could not load schemas from {}: {}", dir.display(), e);
        }
    }

    // Load definitions from same directory as schema file
    if let Some(parent) = cli.schema_file.parent() {
        let _ = registry.load_directory(parent);
    }

    // Resolve refs in schema
    let schema = registry.resolve_refs(schema)
        .unwrap_or_else(|e| {
            eprintln!("Error resolving schema references: {}", e);
            process::exit(1);
        });

    // Parse YAML file
    let yaml_str = std::fs::read_to_string(&cli.yaml_file)
        .unwrap_or_else(|e| {
            eprintln!("Error reading {}: {}", cli.yaml_file.display(), e);
            process::exit(1);
        });

    let yaml = parse(&yaml_str, cli.yaml_file.to_str())
        .unwrap_or_else(|e| {
            eprintln!("Error parsing YAML: {}", e);
            process::exit(1);
        });

    // Validate
    let mut ctx = ValidationContext::new();
    if validate_with_registry(&yaml, &schema, &registry, &mut ctx).is_err() {
        // Print errors
        for error in ctx.errors() {
            let diagnostic = DiagnosticMessageBuilder::error("YAML Validation Error")
                .with_code("Q-1-2")  // Schema validation error
                .problem(error.message())
                .add_detail(format!("At path: {}", error.path()))
                .build();

            // TODO: Use ariadne to render with source context
            eprintln!("{:#?}", diagnostic);
        }
        process::exit(1);
    }

    println!("✓ Validation successful");
}
```

## Implementation Plan

**IMPORTANT**: This plan has been revised to use `YamlWithSourceInfo` instead of serde deserialization to ensure YAML 1.2 compatibility and source tracking.

### Step 0: Remove Serde Deserialization (Day 1)

**Files**:
- `quarto-yaml-validation/src/schema.rs`

**Work**:
1. **Remove** the `impl<'de> Deserialize<'de> for Schema` block (lines 269-572)
2. Remove the `SchemaVisitor` struct
3. Remove all serde deserialization tests (lines 608-919)
4. Update the comment at the top explaining why we don't use serde
5. Add dependency on `quarto-yaml` in Cargo.toml

**Note**: This is a breaking change but necessary before building on top of it.

### Step 1: Add Schema::from_yaml() Method (Week 1, Days 1-3)

**Files**:
- `quarto-yaml-validation/src/schema.rs`
- `quarto-yaml-validation/src/error.rs` (new file for SchemaError)

**Work**:
1. Create `SchemaError` enum with location tracking
2. Implement `Schema::from_yaml()` method
3. Implement helper methods:
   - `parse_short_form()`
   - `parse_object_form()`
   - `parse_inline_enum()`
4. Implement type-specific parsers:
   - `parse_boolean_schema()`
   - `parse_number_schema()`
   - `parse_string_schema()`
   - `parse_null_schema()`
   - `parse_enum_schema()`
   - `parse_any_schema()`
   - `parse_anyof_schema()`
   - `parse_allof_schema()`
   - `parse_array_schema()`
   - `parse_object_schema()`
   - `parse_ref_schema()`
5. Implement annotation parsing and helper methods
6. Add comprehensive tests

**Tests**:
```rust
#[test]
fn test_from_yaml_boolean_short() {
    let yaml = quarto_yaml::parse("boolean", None).unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();
    assert!(matches!(schema, Schema::Boolean(_)));
}

#[test]
fn test_from_yaml_enum_inline() {
    let yaml = quarto_yaml::parse("[foo, bar, baz]", None).unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();
    if let Schema::Enum(e) = schema {
        assert_eq!(e.values.len(), 3);
    } else {
        panic!("Expected Enum schema");
    }
}

#[test]
fn test_from_yaml_object_complex() {
    let yaml = quarto_yaml::parse(r#"
object:
  properties:
    foo: string
    bar: number
  required: [foo]
  closed: true
"#, None).unwrap();
    let schema = Schema::from_yaml(&yaml).unwrap();
    // ... assertions
}

#[test]
fn test_error_includes_location() {
    let yaml = quarto_yaml::parse("invalid_type", None).unwrap();
    let result = Schema::from_yaml(&yaml);
    assert!(result.is_err());
    let err = result.unwrap_err();
    // Verify error includes source location
}
```

### Step 2: Add SchemaField and File Loading (Week 1, Days 4-5)

**Files**:
- `quarto-yaml-validation/src/schema_file.rs` (new)
- `quarto-yaml-validation/src/lib.rs` (expose new module)

**Work**:
1. Define `SchemaField` struct (using YamlWithSourceInfo for values)
2. Define `Description` enum
3. Implement `SchemaField::from_yaml()`
4. Implement `load_schema_file()` using `quarto_yaml::parse_file()`
5. Handle both explicit (`schema:` field) and inline schema syntaxes
6. Test with actual quarto-cli schema files

**Tests**:
```rust
#[test]
fn test_load_document_execute() {
    let path = Path::new("tests/fixtures/document-execute.yml");
    let fields = load_schema_file(path).unwrap();

    assert!(fields.len() > 0);

    // Find "cache" field
    let cache = fields.iter()
        .find(|f| f.name.as_ref() == Some(&"cache".to_string()))
        .unwrap();

    assert!(cache.description.is_some());
    // Note: default is now YamlWithSourceInfo, not a typed value
}

#[test]
fn test_explicit_schema_syntax() {
    let yaml = quarto_yaml::parse(r#"
- name: engine
  schema:
    string:
      completions: [jupyter, knitr]
  description: "Engine for code execution"
"#, None).unwrap();

    let fields: Vec<SchemaField> = yaml.as_array().unwrap()
        .iter()
        .map(|item| SchemaField::from_yaml(item).unwrap())
        .collect();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, Some("engine".to_string()));
}

#[test]
fn test_inline_schema_syntax() {
    let yaml = quarto_yaml::parse(r#"
- id: math-methods
  enum:
    values: [plain, webtex, gladtex]
  description: "Math rendering method"
"#, None).unwrap();

    let fields: Vec<SchemaField> = yaml.as_array().unwrap()
        .iter()
        .map(|item| SchemaField::from_yaml(item).unwrap())
        .collect();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].id, Some("math-methods".to_string()));
}
```

### Step 3: Add Schema Registry (Week 2)

**Files**:
- `quarto-yaml-validation/src/registry.rs` (new)

**Work**:
1. Implement `SchemaRegistry`
2. Implement `load_file()`, `load_directory()`
3. Implement `resolve_refs()` (recursive)
4. Test with definitions.yml and cross-file refs

### Step 4: Update Validation to Support Registry (Week 2)

**Files**:
- `quarto-yaml-validation/src/validate.rs`

**Work**:
1. Add `validate_with_registry()` function
2. Resolve refs during validation
3. Handle unresolved ref errors
4. Update tests

### Step 5: Create validate-yaml Binary (Week 2)

**Files**:
- `crates/validate-yaml/` (new crate)

**Work**:
1. Set up binary crate
2. Implement CLI with clap
3. Wire up parsing, schema loading, validation
4. Format errors with quarto-error-reporting
5. Add to workspace

### Step 6: Integration Testing (Week 2)

**Work**:
1. Test with real quarto-cli schema files
2. Test validation of real quarto documents
3. Verify error messages are helpful
4. Document usage

## Timeline Estimate

**Total**: 2-3 weeks (revised for YamlWithSourceInfo approach)

- **Week 1**:
  - Days 1-3: Remove serde, implement Schema::from_yaml() with all type parsers
  - Days 4-5: SchemaField and file loading
- **Week 2**:
  - Days 1-2: Schema registry with ref resolution
  - Day 3: Update validation to support registry
  - Days 4-5: Create validate-yaml binary crate
- **Week 3**: Integration testing, polish, documentation

**Note**: The YamlWithSourceInfo approach is more manual than serde deserialization but provides better error messages and YAML 1.2 compatibility. Estimate is similar because we save time on not fighting with serde's limitations.

## Benefits

1. **YAML 1.2 Compatibility**: Consistent parsing for user documents and schemas
2. **Source Tracking**: Every schema element has source location for better error messages
3. **Extensions Support**: Same infrastructure for Quarto core and extensions
4. **Dogfooding**: Use our own validation system to validate schemas
5. **Testing Lab**: Isolated binary to experiment with APIs
6. **Foundation**: Infrastructure for schema-driven validation in main CLI
7. **Compatibility**: Direct compatibility with quarto-cli schemas
8. **Incremental**: Can start with subset of schema features
9. **No serde_yaml dependency**: One less dependency with YAML 1.1 limitations

## Open Questions

1. **MaybeArrayOf handling**: This is Quarto-specific. Keep as separate variant or normalize to `AnyOf([T, ArrayOf(T)])`?
   - **Decision needed**: Affects quarto-cli compatibility
2. **Record type**: How does it differ from Object in practice? Need more examples from quarto-cli.
3. **Schema versioning**: Should we track which version of schema format we support?
4. **Performance**: Resolve refs once upfront or lazily during validation?
5. **Inline schema parsing**: The `parse_inline_schema()` method needs a clean way to construct a new YamlWithSourceInfo with just the schema fields filtered from metadata. May need helper method in quarto-yaml.

## Next Steps

1. Get approval on design
2. Start with Step 1 (deserialization)
3. Test incrementally with quarto-cli schemas
4. Build binary once deserialization works
