# YAML Validation Rust Design

**Date**: 2025-10-13
**Topic**: Designing a Rust crate for YAML validation with source tracking
**Status**: Design proposal

## Executive Summary

**Goal**: Port Quarto's YAML validation system to Rust, maintaining its excellent error messages while integrating with our `YamlWithSourceInfo` (quarto-cli's `AnnotatedParse`).

**Key insight**: Quarto's validator uses a simplified subset of JSON Schema, sacrificing some features for dramatically better error messages. The system is tightly integrated with source location tracking for precise error reporting.

**Recommendation**: Create `quarto-yaml-validation` crate with three main components:
1. **Schema types** - Enum-based schema representation
2. **Validator** - Validates YAML against schemas with context tracking
3. **Schema compiler** - Compiles YAML schema definitions to runtime schemas

**Estimated effort**: 6-8 weeks (parallelizable with other work)

## Current TypeScript Architecture

### Overview

```
┌─────────────────────────────────────────────────────────────────┐
│              YAML Schema Definitions (*.yml)                     │
│  Cell/document properties with name/schema/description/tags     │
└─────────────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────────────┐
│              from-yaml.ts: Schema Compiler                       │
│  convertFromYaml(): Converts YAML → internal Schema types       │
└─────────────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────────────┐
│              types.ts: Schema Type Definitions                   │
│  BooleanSchema, NumberSchema, ObjectSchema, AnyOfSchema, etc.   │
└─────────────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────────────┐
│              validator.ts: Validation Engine                     │
│  validate(value: AnnotatedParse, schema: Schema) → errors       │
└─────────────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────────────┐
│              errors.ts: Error Improvement                        │
│  Error handlers: typo suggestions, better messages, context     │
└─────────────────────────────────────────────────────────────────┘
```

### Key Files Analyzed

| File | LOC | Purpose |
|------|-----|---------|
| `yaml-validation/validator.ts` | ~750 | Core validation logic |
| `yaml-validation/errors.ts` | ~1000 | Error creation and improvement |
| `yaml-schema/types.ts` | ~330 | Schema type definitions |
| `yaml-schema/from-yaml.ts` | ~750 | Schema compiler |
| `resources/schema/*.yml` | ~4000 | Schema definitions |

**Total**: ~7000 LOC (excluding schema YAMLs)

### Schema Type Hierarchy

```typescript
type Schema =
  | FalseSchema        // false (never valid)
  | TrueSchema         // true (always valid)
  | BooleanSchema      // { type: "boolean" }
  | NumberSchema       // { type: "number", minimum?, maximum?, ... }
  | StringSchema       // { type: "string", pattern? }
  | NullSchema         // { type: "null" }
  | EnumSchema         // { type: "enum", enum: [...] }
  | AnySchema          // { type: "any" } (Quarto extension)
  | AnyOfSchema        // { type: "anyOf", anyOf: [...] }
  | AllOfSchema        // { type: "allOf", allOf: [...] }
  | ArraySchema        // { type: "array", items?, minItems?, ... }
  | ObjectSchema       // { type: "object", properties?, required?, ... }
  | RefSchema          // { type: "ref", $ref: "..." }

interface SchemaAnnotations {
  $id?: string;
  documentation?: string | { short: string; long: string };
  description?: string;         // For error messages
  errorMessage?: string;         // Custom error message
  hidden?: boolean;             // Hide in completions
  completions?: string[];       // IDE completions
  tags?: Record<string, unknown>; // Arbitrary metadata
}
```

### AnnotatedParse (YamlWithSourceInfo equivalent)

```typescript
interface AnnotatedParse {
  start: number;              // Start position in source
  end: number;                // End position in source
  result: JSONValue;          // Parsed value
  kind: string;               // "mapping", "sequence", "scalar", etc.
  source: MappedString;       // Source with location tracking
  components: AnnotatedParse[]; // Child nodes (keys + values for mappings)
}
```

**Critical observation**: Our `YamlWithSourceInfo` is almost identical!

```rust
pub struct YamlWithSourceInfo {
    pub value: Yaml,              // = result
    pub source_info: SourceInfo,  // = start/end + source
    pub children: Children,       // = components
}
```

### Validation Flow

```typescript
// 1. Create validation context
const context = new ValidationContext();

// 2. Validate recursively
function validateGeneric(value: AnnotatedParse, schema: Schema, context) {
  // Dispatch based on schema type
  switch (schemaType(schema)) {
    case "boolean": return validateBoolean(value, schema, context);
    case "number": return validateNumber(value, schema, context);
    case "object": return validateObject(value, schema, context);
    // ... etc
  }
}

// 3. For objects: navigate to find keys/values
function validateObject(value: AnnotatedParse, schema: ObjectSchema, context) {
  // Check required properties
  for (const reqKey of schema.required) {
    if (!hasProperty(value.result, reqKey)) {
      context.error(value, schema, `missing required property ${reqKey}`);
    }
  }

  // Validate each property
  for (const [key, subSchema] of Object.entries(schema.properties)) {
    const propValue = locate(key); // Find in components
    validateGeneric(propValue, subSchema, context);
  }
}

// 4. Collect and improve errors
const errors = context.collectErrors(schema, source, value);
for (const error of errors) {
  // Apply error handlers to improve messages
  error = checkForTypeMismatch(error);
  error = checkForNearbyCorrection(error);  // Typo suggestions
  error = expandEmptySpan(error);           // Better highlighting
}
```

### Navigate Function (Critical!)

```typescript
// Navigates through AnnotatedParse tree using instance path
function navigate(
  path: (number | string)[],
  annotation: AnnotatedParse,
  returnKey = false,  // Return key instead of value
  pathIndex = 0,
): AnnotatedParse | undefined {
  if (pathIndex >= path.length) return annotation;

  if (annotation.kind === "mapping") {
    const { components } = annotation;
    const searchKey = path[pathIndex];

    // Loop backwards for duplicate key handling
    for (let i = components.length - 2; i >= 0; i -= 2) {
      const key = components[i].result;
      if (key === searchKey) {
        if (returnKey && pathIndex === path.length - 1) {
          return navigate(path, components[i], returnKey, pathIndex + 1);
        } else {
          return navigate(path, components[i + 1], returnKey, pathIndex + 1);
        }
      }
    }
  } else if (annotation.kind === "sequence") {
    const index = Number(path[pathIndex]);
    return navigate(path, annotation.components[index], returnKey, pathIndex + 1);
  }

  return annotation;
}
```

**Why this matters**: Validation needs to find specific keys/values to:
1. Check if required properties exist
2. Validate property values against schemas
3. Create errors pointing to exact locations

### Error Handling

**Error handlers** (applied after validation):

1. **`ignoreExprViolations`**: Skip errors for `!expr` tags (R expressions)
2. **`expandEmptySpan`**: Highlight key instead of empty value
3. **`improveErrorHeadingForValueErrors`**: Better error messages
4. **`checkForTypeMismatch`**: "value is of type X" messages
5. **`checkForBadBoolean`**: Detect YAML 1.0 booleans (`yes`/`no`)
6. **`checkForBadColon`**: Detect missing space (`key:value` → `key: value`)
7. **`checkForBadEquals`**: Detect `=` instead of `:` in objects
8. **`checkForNearbyCorrection`**: Suggest typo corrections (edit distance)
9. **`checkForNearbyRequired`**: Suggest typos in required field names
10. **`schemaDefinedErrors`**: Use custom `errorMessage` from schema

**Error pruning for `anyOf`**: When validation fails on `anyOf`, pick the "best" error:
- Prefer "missing required field" over "invalid field name"
- Prefer errors with smallest span (easier to fix)
- Use `error-importance` tag from schema

### Schema Compilation

Schema YAMLs use a simplified syntax:

```yaml
- name: code-copy
  schema:
    anyOf:
      - enum: [hover]
      - boolean
  default: "hover"
  description:
    short: Enable a code copy icon
    long: |
      Enable a code copy icon for code blocks.
      - `true`: Always show the icon
      - `false`: Never show the icon
```

Compiler converts to:

```typescript
{
  type: "anyOf",
  anyOf: [
    { type: "enum", enum: ["hover"] },
    { type: "boolean" }
  ],
  documentation: { short: "...", long: "..." },
  ...
}
```

**Key features**:
- `maybeArrayOf: T` → `anyOf: [T, arrayOf: T]`
- `object.closed: true` → Disallow extra properties
- `object.required: "all"` → All properties required
- `ref: "schema-id"` → Reference to defined schema
- `pattern: "regex"` → String pattern validation

## Proposed Rust Design

### Crate Structure

```
quarto-yaml-validation/
├── src/
│   ├── lib.rs              // Public API
│   ├── schema/
│   │   ├── mod.rs          // Schema types
│   │   ├── types.rs        // Schema enum + annotations
│   │   ├── compiler.rs     // YAML → Schema compilation
│   │   └── registry.rs     // Schema registry ($ref resolution)
│   ├── validator/
│   │   ├── mod.rs          // Validation entry point
│   │   ├── context.rs      // ValidationContext
│   │   ├── validate.rs     // Type-specific validators
│   │   └── navigate.rs     // Navigate function
│   ├── error/
│   │   ├── mod.rs          // Error types
│   │   ├── handlers.rs     // Error improvement handlers
│   │   └── formatting.rs   // Error message formatting
│   └── util/
│       └── edit_distance.rs // For typo suggestions
└── tests/
    ├── validation_tests.rs
    └── fixtures/
        └── schemas/*.yml
```

### Core Type Definitions

```rust
// ============================================================================
// SCHEMA TYPES
// ============================================================================

#[derive(Debug, Clone)]
pub enum Schema {
    False,  // Never valid
    True,   // Always valid
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

#[derive(Debug, Clone, Default)]
pub struct SchemaAnnotations {
    pub id: Option<String>,
    pub documentation: Option<Documentation>,
    pub description: Option<String>,      // For error messages
    pub error_message: Option<String>,    // Custom error text
    pub hidden: bool,                     // Hide in completions
    pub completions: Vec<String>,         // IDE suggestions
    pub tags: HashMap<String, serde_json::Value>, // Arbitrary metadata
}

#[derive(Debug, Clone)]
pub enum Documentation {
    Short(String),
    Long { short: String, long: String },
}

#[derive(Debug, Clone)]
pub struct BooleanSchema {
    pub annotations: SchemaAnnotations,
}

#[derive(Debug, Clone)]
pub struct NumberSchema {
    pub annotations: SchemaAnnotations,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub exclusive_minimum: Option<f64>,
    pub exclusive_maximum: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct StringSchema {
    pub annotations: SchemaAnnotations,
    pub pattern: Option<String>,
    pub compiled_pattern: Option<Regex>,  // Cached
}

#[derive(Debug, Clone)]
pub struct EnumSchema {
    pub annotations: SchemaAnnotations,
    pub values: Vec<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct AnyOfSchema {
    pub annotations: SchemaAnnotations,
    pub schemas: Vec<Schema>,
}

#[derive(Debug, Clone)]
pub struct AllOfSchema {
    pub annotations: SchemaAnnotations,
    pub schemas: Vec<Schema>,
}

#[derive(Debug, Clone)]
pub struct ArraySchema {
    pub annotations: SchemaAnnotations,
    pub items: Option<Box<Schema>>,
    pub min_items: Option<usize>,
    pub max_items: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ObjectSchema {
    pub annotations: SchemaAnnotations,
    pub properties: Option<HashMap<String, Schema>>,
    pub pattern_properties: Option<HashMap<String, Schema>>,
    pub compiled_patterns: Option<HashMap<String, Regex>>,  // Cached
    pub required: Vec<String>,
    pub additional_properties: Option<Box<Schema>>,
    pub property_names: Option<Box<Schema>>,
    pub closed: bool,  // Quarto extension: disallow extra properties
}

#[derive(Debug, Clone)]
pub struct RefSchema {
    pub annotations: SchemaAnnotations,
    pub reference: String,  // e.g., "quarto-resource-document-code-highlight"
}

// ============================================================================
// VALIDATION TYPES
// ============================================================================

pub type InstancePath = Vec<PathSegment>;
pub type SchemaPath = Vec<PathSegment>;

#[derive(Debug, Clone)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub value: YamlWithSourceInfo,
    pub schema: Schema,
    pub message: String,
    pub instance_path: InstancePath,
    pub schema_path: SchemaPath,
}

#[derive(Debug, Clone)]
pub struct LocalizedError {
    pub violating_object: YamlWithSourceInfo,
    pub schema: Schema,
    pub message: String,
    pub instance_path: InstancePath,
    pub schema_path: SchemaPath,
    pub source: MappedString,
    pub location: SourceLocation,
    pub nice_error: TidyverseError,  // From quarto-errors crate
}

// ============================================================================
// VALIDATION CONTEXT
// ============================================================================

pub struct ValidationContext {
    instance_path: InstancePath,
    schema_path: SchemaPath,
    root: ValidationTraceNode,
    node_stack: Vec<ValidationTraceNode>,
}

#[derive(Debug, Clone)]
struct ValidationTraceNode {
    edge: PathSegment,
    errors: Vec<ValidationError>,
    children: Vec<ValidationTraceNode>,
}

impl ValidationContext {
    pub fn new() -> Self {
        let root = ValidationTraceNode {
            edge: PathSegment::Key("#".to_string()),
            errors: Vec::new(),
            children: Vec::new(),
        };

        Self {
            instance_path: Vec::new(),
            schema_path: Vec::new(),
            root: root.clone(),
            node_stack: vec![root],
        }
    }

    pub fn error(&mut self, value: &YamlWithSourceInfo, schema: &Schema, message: String) {
        let current = self.node_stack.last_mut().unwrap();
        current.errors.push(ValidationError {
            value: value.clone(),
            schema: schema.clone(),
            message,
            instance_path: self.instance_path.clone(),
            schema_path: self.schema_path.clone(),
        });
    }

    pub fn push_schema(&mut self, segment: PathSegment) {
        let new_node = ValidationTraceNode {
            edge: segment.clone(),
            errors: Vec::new(),
            children: Vec::new(),
        };

        let current = self.node_stack.last_mut().unwrap();
        current.children.push(new_node.clone());

        self.node_stack.push(new_node);
        self.schema_path.push(segment);
    }

    pub fn pop_schema(&mut self, success: bool) {
        self.node_stack.pop();
        self.schema_path.pop();

        if success {
            // Remove the child we just added since validation passed
            let current = self.node_stack.last_mut().unwrap();
            current.children.pop();
        }
    }

    pub fn push_instance(&mut self, segment: PathSegment) {
        self.instance_path.push(segment);
    }

    pub fn pop_instance(&mut self) {
        self.instance_path.pop();
    }

    pub fn with_schema_path<F>(&mut self, segment: PathSegment, f: F) -> bool
    where
        F: FnOnce(&mut Self) -> bool,
    {
        self.push_schema(segment);
        let result = f(self);
        self.pop_schema(result);
        result
    }
}
```

### Navigate Function

```rust
/// Navigate through YamlWithSourceInfo tree using instance path
pub fn navigate<'a>(
    path: &InstancePath,
    annotation: &'a YamlWithSourceInfo,
    return_key: bool,
    path_index: usize,
) -> Result<&'a YamlWithSourceInfo> {
    if path_index >= path.len() {
        return Ok(annotation);
    }

    match &annotation.children {
        Children::Mapping(entries) => {
            if let PathSegment::Key(search_key) = &path[path_index] {
                // Loop backwards for duplicate key handling (last wins)
                for entry in entries.iter().rev() {
                    if let Yaml::String(key) = &entry.key.value {
                        if key == search_key {
                            if return_key && path_index == path.len() - 1 {
                                return navigate(path, &entry.key, return_key, path_index + 1);
                            } else {
                                return navigate(path, &entry.value, return_key, path_index + 1);
                            }
                        }
                    }
                }

                bail!("Key '{}' not found in mapping", search_key);
            } else {
                bail!("Expected key segment, got index");
            }
        }

        Children::Sequence(items) => {
            if let PathSegment::Index(index) = path[path_index] {
                if index < items.len() {
                    return navigate(path, &items[index], return_key, path_index + 1);
                } else {
                    bail!("Index {} out of bounds (len = {})", index, items.len());
                }
            } else {
                bail!("Expected index segment, got key");
            }
        }

        Children::Scalar => {
            // Can't navigate further
            Ok(annotation)
        }
    }
}

/// Shorthand for navigate without returnKey
pub fn navigate_to_value<'a>(
    path: &InstancePath,
    annotation: &'a YamlWithSourceInfo,
) -> Result<&'a YamlWithSourceInfo> {
    navigate(path, annotation, false, 0)
}

/// Navigate and return key instead of value
pub fn navigate_to_key<'a>(
    path: &InstancePath,
    annotation: &'a YamlWithSourceInfo,
) -> Result<&'a YamlWithSourceInfo> {
    navigate(path, annotation, true, 0)
}
```

### Validator Implementation

```rust
// ============================================================================
// MAIN VALIDATION FUNCTION
// ============================================================================

pub fn validate(
    value: &YamlWithSourceInfo,
    schema: &Schema,
    source: &MappedString,
    prune_errors: bool,
) -> Result<Vec<LocalizedError>> {
    let mut context = ValidationContext::new();
    validate_generic(value, schema, &mut context)?;

    let errors = context.collect_errors(schema, source, value, prune_errors)?;

    // Apply error improvement handlers
    let improved_errors = errors.into_iter()
        .filter_map(|e| improve_error(e, value, schema))
        .collect();

    Ok(improved_errors)
}

fn validate_generic(
    value: &YamlWithSourceInfo,
    schema: &Schema,
    context: &mut ValidationContext,
) -> bool {
    match schema {
        Schema::False => {
            context.error(value, schema, "false schema never validates".to_string());
            false
        }
        Schema::True => true,
        Schema::Boolean(s) => validate_boolean(value, s, context),
        Schema::Number(s) => validate_number(value, s, context),
        Schema::String(s) => validate_string(value, s, context),
        Schema::Null(s) => validate_null(value, s, context),
        Schema::Enum(s) => validate_enum(value, s, context),
        Schema::Any(_) => true,  // Always passes
        Schema::AnyOf(s) => validate_any_of(value, s, context),
        Schema::AllOf(s) => validate_all_of(value, s, context),
        Schema::Array(s) => validate_array(value, s, context),
        Schema::Object(s) => validate_object(value, s, context),
        Schema::Ref(s) => {
            // Resolve reference and validate
            let resolved = resolve_schema_ref(&s.reference)?;
            validate_generic(value, &resolved, context)
        }
    }
}

// ============================================================================
// TYPE-SPECIFIC VALIDATORS
// ============================================================================

fn validate_boolean(
    value: &YamlWithSourceInfo,
    schema: &BooleanSchema,
    context: &mut ValidationContext,
) -> bool {
    match &value.value {
        Yaml::Boolean(_) => true,
        _ => {
            context.with_schema_path(PathSegment::Key("type".to_string()), |ctx| {
                ctx.error(value, &Schema::Boolean(schema.clone()), "type mismatch".to_string());
                false
            })
        }
    }
}

fn validate_number(
    value: &YamlWithSourceInfo,
    schema: &NumberSchema,
    context: &mut ValidationContext,
) -> bool {
    let num = match value.value {
        Yaml::Real(ref s) => s.parse::<f64>().ok(),
        Yaml::Integer(i) => Some(i as f64),
        _ => None,
    };

    let Some(num) = num else {
        return context.with_schema_path(PathSegment::Key("type".to_string()), |ctx| {
            ctx.error(value, &Schema::Number(schema.clone()), "type mismatch".to_string());
            false
        });
    };

    let mut result = true;

    if let Some(min) = schema.minimum {
        result &= context.with_schema_path(PathSegment::Key("minimum".to_string()), |ctx| {
            if num < min {
                ctx.error(
                    value,
                    &Schema::Number(schema.clone()),
                    format!("value {} is less than required minimum {}", num, min),
                );
                false
            } else {
                true
            }
        });
    }

    if let Some(max) = schema.maximum {
        result &= context.with_schema_path(PathSegment::Key("maximum".to_string()), |ctx| {
            if num > max {
                ctx.error(
                    value,
                    &Schema::Number(schema.clone()),
                    format!("value {} is greater than required maximum {}", num, max),
                );
                false
            } else {
                true
            }
        });
    }

    result
}

fn validate_object(
    value: &YamlWithSourceInfo,
    schema: &ObjectSchema,
    context: &mut ValidationContext,
) -> bool {
    // Check if value is actually an object
    let Children::Mapping(entries) = &value.children else {
        return context.with_schema_path(PathSegment::Key("type".to_string()), |ctx| {
            ctx.error(value, &Schema::Object(schema.clone()), "type mismatch".to_string());
            false
        });
    };

    let mut result = true;

    // Build set of property keys
    let mut own_properties = HashSet::new();
    for entry in entries {
        if let Yaml::String(key) = &entry.key.value {
            own_properties.insert(key.clone());
        }
    }

    // Check required properties
    if !schema.required.is_empty() {
        result &= context.with_schema_path(PathSegment::Key("required".to_string()), |ctx| {
            let mut ok = true;
            for req_key in &schema.required {
                if !own_properties.contains(req_key) {
                    ctx.error(
                        value,
                        &Schema::Object(schema.clone()),
                        format!("object is missing required property {}", req_key),
                    );
                    ok = false;
                }
            }
            ok
        });
    }

    // Validate properties
    if let Some(ref properties) = schema.properties {
        result &= context.with_schema_path(PathSegment::Key("properties".to_string()), |ctx| {
            let mut ok = true;
            for (key, sub_schema) in properties {
                if own_properties.contains(key) {
                    // Find value in components
                    let path = vec![PathSegment::Key(key.clone())];
                    if let Ok(prop_value) = navigate_to_value(&path, value) {
                        ctx.push_instance(PathSegment::Key(key.clone()));
                        ok &= ctx.with_schema_path(PathSegment::Key(key.clone()), |ctx2| {
                            validate_generic(prop_value, sub_schema, ctx2)
                        });
                        ctx.pop_instance();
                    }
                }
            }
            ok
        });
    }

    // Check closed schema (no extra properties)
    if schema.closed {
        result &= context.with_schema_path(PathSegment::Key("closed".to_string()), |ctx| {
            let allowed_keys: HashSet<_> = schema.properties
                .as_ref()
                .map(|p| p.keys().cloned().collect())
                .unwrap_or_default();

            let mut ok = true;
            for key in &own_properties {
                if !allowed_keys.contains(key) {
                    // Find the key node for error reporting
                    let path = vec![PathSegment::Key(key.clone())];
                    if let Ok(key_node) = navigate_to_key(&path, value) {
                        ctx.error(
                            key_node,
                            &Schema::Object(schema.clone()),
                            format!("object has invalid field {}", key),
                        );
                    }
                    ok = false;
                }
            }
            ok
        });
    }

    result
}

fn validate_any_of(
    value: &YamlWithSourceInfo,
    schema: &AnyOfSchema,
    context: &mut ValidationContext,
) -> bool {
    let mut passing = 0;

    for (i, sub_schema) in schema.schemas.iter().enumerate() {
        if context.with_schema_path(PathSegment::Index(i), |ctx| {
            validate_generic(value, sub_schema, ctx)
        }) {
            passing += 1;
        }
    }

    passing > 0
}

fn validate_all_of(
    value: &YamlWithSourceInfo,
    schema: &AllOfSchema,
    context: &mut ValidationContext,
) -> bool {
    let mut passing = 0;

    for (i, sub_schema) in schema.schemas.iter().enumerate() {
        if context.with_schema_path(PathSegment::Index(i), |ctx| {
            validate_generic(value, sub_schema, ctx)
        }) {
            passing += 1;
        }
    }

    passing == schema.schemas.len()
}
```

### Error Collection and Pruning

```rust
impl ValidationContext {
    pub fn collect_errors(
        &self,
        _schema: &Schema,
        source: &MappedString,
        _value: &YamlWithSourceInfo,
        prune_errors: bool,
    ) -> Result<Vec<LocalizedError>> {
        let errors = self.collect_errors_inner(&self.root, prune_errors);

        errors.into_iter()
            .map(|e| create_localized_error(e, source))
            .collect()
    }

    fn collect_errors_inner(
        &self,
        node: &ValidationTraceNode,
        prune_errors: bool,
    ) -> Vec<ValidationError> {
        let mut result = Vec::new();

        // Special handling for anyOf to pick best error
        if matches!(node.edge, PathSegment::Key(ref k) if k == "anyOf") && prune_errors {
            let inner_results: Vec<_> = node.children.iter()
                .map(|child| self.collect_errors_inner(child, prune_errors))
                .collect();

            // Heuristic: prefer "missing required field" over "invalid field name"
            let has_required_error = inner_results.iter()
                .any(|errors| errors.iter().any(|e| {
                    matches!(e.schema_path.last(), Some(PathSegment::Key(k)) if k == "required")
                }));

            let has_property_names_error = inner_results.iter()
                .any(|errors| errors.iter().any(|e| {
                    e.schema_path.iter().any(|seg| {
                        matches!(seg, PathSegment::Key(k) if k == "propertyNames")
                    })
                }));

            if has_required_error && has_property_names_error {
                // Prefer required error
                return inner_results.into_iter()
                    .filter(|errors| {
                        errors.iter().any(|e| {
                            matches!(e.schema_path.last(), Some(PathSegment::Key(k)) if k == "required")
                        })
                    })
                    .flatten()
                    .collect();
            }

            // Otherwise, pick error with smallest span
            if let Some(best) = inner_results.into_iter()
                .min_by_key(|errors| {
                    errors.iter().map(|e| {
                        e.value.source_info.span().map(|s| s.len()).unwrap_or(0)
                    }).sum::<usize>()
                }) {
                return best;
            }
        } else {
            result.extend(node.errors.clone());
            for child in &node.children {
                result.extend(self.collect_errors_inner(child, prune_errors));
            }
        }

        result
    }
}

fn create_localized_error(
    error: ValidationError,
    source: &MappedString,
) -> Result<LocalizedError> {
    let location = error.value.source_info.to_source_location()?;

    let nice_error = TidyverseError {
        heading: error.message.clone(),
        error: Vec::new(),
        info: HashMap::new(),
        file_name: location.file_name.clone(),
        location: location.clone(),
        source_context: create_source_context(&error.value, source)?,
    };

    Ok(LocalizedError {
        violating_object: error.value,
        schema: error.schema,
        message: error.message,
        instance_path: error.instance_path,
        schema_path: error.schema_path,
        source: source.clone(),
        location,
        nice_error,
    })
}
```

### Error Improvement Handlers

```rust
// ============================================================================
// ERROR IMPROVEMENT
// ============================================================================

pub fn improve_error(
    mut error: LocalizedError,
    parse: &YamlWithSourceInfo,
    schema: &Schema,
) -> Option<LocalizedError> {
    // Chain of handlers
    error = ignore_expr_violations(error)?;
    error = expand_empty_span(error, parse)?;
    error = improve_error_heading(error, parse, schema)?;
    error = check_for_type_mismatch(error, parse, schema)?;
    error = check_for_bad_boolean(error, parse, schema)?;
    error = check_for_bad_colon(error, parse, schema)?;
    error = check_for_nearby_correction(error, parse, schema)?;
    error = check_for_nearby_required(error, parse, schema)?;
    error = apply_schema_defined_errors(error)?;

    Some(error)
}

fn check_for_nearby_correction(
    mut error: LocalizedError,
    parse: &YamlWithSourceInfo,
    schema: &Schema,
) -> Option<LocalizedError> {
    // Check if this is a key error or value error
    let (err_val, corrections) = if let Some(bad_key) = get_bad_key(&error) {
        (bad_key, possible_schema_keys(schema))
    } else {
        let val = navigate_to_value(&error.instance_path, parse).ok()?;
        match &val.value {
            Yaml::String(s) => (s.clone(), possible_schema_values(schema)),
            _ => return Some(error),
        }
    };

    if corrections.is_empty() {
        return Some(error);
    }

    // Find best correction using edit distance
    let mut best_corrections = Vec::new();
    let mut best_distance = usize::MAX;

    for correction in corrections {
        let distance = edit_distance(&err_val, &correction);
        if distance < best_distance {
            best_corrections = vec![correction];
            best_distance = distance;
        } else if distance == best_distance {
            best_corrections.push(correction);
        }
    }

    // Only suggest if edit distance < 30% of word length
    if best_distance > (err_val.len() * 3) / 10 {
        return Some(error);
    }

    // Add suggestion to error
    let suggestion = if best_corrections.len() == 1 {
        format!("Did you mean '{}'?", best_corrections[0])
    } else if best_corrections.len() == 2 {
        format!("Did you mean '{}' or '{}'?", best_corrections[0], best_corrections[1])
    } else {
        format!("Did you mean one of: {}?", best_corrections.join(", "))
    };

    error.nice_error.info.insert("did-you-mean".to_string(), suggestion);
    Some(error)
}

fn check_for_bad_boolean(
    mut error: LocalizedError,
    _parse: &YamlWithSourceInfo,
    schema: &Schema,
) -> Option<LocalizedError> {
    // Check if expecting boolean but got string that looks like YAML 1.0 boolean
    if !matches!(schema, Schema::Boolean(_)) {
        return Some(error);
    }

    let Yaml::String(ref s) = error.violating_object.value else {
        return Some(error);
    };

    let yaml_10_booleans = [
        "y", "Y", "yes", "Yes", "YES", "true", "True", "TRUE", "on", "On", "ON",
        "n", "N", "no", "No", "NO", "false", "False", "FALSE", "off", "Off", "OFF"
    ];

    if !yaml_10_booleans.contains(&s.as_str()) {
        return Some(error);
    }

    let fix = if ["y", "Y", "yes", "Yes", "YES", "true", "True", "TRUE", "on", "On", "ON"].contains(&s.as_str()) {
        "true"
    } else {
        "false"
    };

    error.nice_error.heading = format!("The value '{}' is a string, not a boolean", s);
    error.nice_error.info.insert(
        "yaml-version-1.2".to_string(),
        "Quarto uses YAML 1.2, which interprets booleans strictly.".to_string(),
    );
    error.nice_error.info.insert(
        "suggestion-fix".to_string(),
        format!("Try using '{}' instead", fix),
    );

    Some(error)
}
```

### Schema Compilation

```rust
// ============================================================================
// SCHEMA COMPILATION FROM YAML
// ============================================================================

pub fn compile_schema_from_yaml(yaml_content: &str) -> Result<Schema> {
    let yaml: serde_yaml::Value = serde_yaml::from_str(yaml_content)?;
    convert_from_yaml(&yaml)
}

fn convert_from_yaml(yaml: &serde_yaml::Value) -> Result<Schema> {
    use serde_yaml::Value as Y;

    // Handle literal types
    match yaml {
        Y::String(s) => match s.as_str() {
            "string" => return Ok(Schema::String(StringSchema::default())),
            "number" => return Ok(Schema::Number(NumberSchema::default())),
            "boolean" => return Ok(Schema::Boolean(BooleanSchema::default())),
            "any" => return Ok(Schema::Any(AnySchema::default())),
            "path" => return Ok(Schema::String(StringSchema::default())),  // FIXME: track this
            "null" => return Ok(Schema::Null(NullSchema::default())),
            _ => {
                // Single-value enum
                return Ok(Schema::Enum(EnumSchema {
                    annotations: SchemaAnnotations::default(),
                    values: vec![serde_json::Value::String(s.clone())],
                }));
            }
        },

        Y::Number(n) => {
            // Single-value enum
            let val = if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Value::from(f)
            } else {
                bail!("Invalid number in schema");
            };
            return Ok(Schema::Enum(EnumSchema {
                annotations: SchemaAnnotations::default(),
                values: vec![val],
            }));
        }

        Y::Bool(b) => {
            // Single-value enum
            return Ok(Schema::Enum(EnumSchema {
                annotations: SchemaAnnotations::default(),
                values: vec![serde_json::Value::Bool(*b)],
            }));
        }

        Y::Mapping(map) => {
            // Check for schema types
            if map.contains_key(&Y::String("anyOf".to_string())) {
                return convert_from_any_of(map);
            }
            if map.contains_key(&Y::String("allOf".to_string())) {
                return convert_from_all_of(map);
            }
            if map.contains_key(&Y::String("object".to_string())) {
                return convert_from_object(map);
            }
            if map.contains_key(&Y::String("enum".to_string())) {
                return convert_from_enum(map);
            }
            if map.contains_key(&Y::String("arrayOf".to_string())) {
                return convert_from_array_of(map);
            }
            if map.contains_key(&Y::String("maybeArrayOf".to_string())) {
                return convert_from_maybe_array_of(map);
            }
            if map.contains_key(&Y::String("string".to_string())) {
                return convert_from_string(map);
            }
            if map.contains_key(&Y::String("pattern".to_string())) {
                return convert_from_pattern(map);
            }
            if map.contains_key(&Y::String("ref".to_string())) {
                return convert_from_ref(map);
            }

            bail!("Unknown schema type in mapping");
        }

        _ => bail!("Unsupported YAML type in schema"),
    }
}

fn convert_from_any_of(map: &serde_yaml::Mapping) -> Result<Schema> {
    let any_of_val = map.get(&serde_yaml::Value::String("anyOf".to_string()))
        .ok_or_else(|| anyhow!("anyOf field missing"))?;

    let any_of_array = any_of_val.as_sequence()
        .ok_or_else(|| anyhow!("anyOf must be array"))?;

    let schemas: Result<Vec<_>> = any_of_array.iter()
        .map(convert_from_yaml)
        .collect();

    let mut schema = AnyOfSchema {
        annotations: SchemaAnnotations::default(),
        schemas: schemas?,
    };

    set_base_properties(map, &mut schema.annotations)?;

    Ok(Schema::AnyOf(schema))
}

fn convert_from_object(map: &serde_yaml::Mapping) -> Result<Schema> {
    let object_val = map.get(&serde_yaml::Value::String("object".to_string()))
        .ok_or_else(|| anyhow!("object field missing"))?;

    let object_map = object_val.as_mapping()
        .ok_or_else(|| anyhow!("object must be mapping"))?;

    let mut schema = ObjectSchema {
        annotations: SchemaAnnotations::default(),
        properties: None,
        pattern_properties: None,
        compiled_patterns: None,
        required: Vec::new(),
        additional_properties: None,
        property_names: None,
        closed: false,
    };

    // Parse properties
    if let Some(props_val) = object_map.get(&serde_yaml::Value::String("properties".to_string())) {
        let props_map = props_val.as_mapping()
            .ok_or_else(|| anyhow!("properties must be mapping"))?;

        let mut properties = HashMap::new();
        for (key, value) in props_map {
            let key_str = key.as_str()
                .ok_or_else(|| anyhow!("property key must be string"))?;
            properties.insert(key_str.to_string(), convert_from_yaml(value)?);
        }
        schema.properties = Some(properties);
    }

    // Parse required
    if let Some(req_val) = object_map.get(&serde_yaml::Value::String("required".to_string())) {
        if let Some(req_str) = req_val.as_str() {
            if req_str == "all" {
                // All properties are required
                if let Some(ref props) = schema.properties {
                    schema.required = props.keys().cloned().collect();
                }
            }
        } else if let Some(req_array) = req_val.as_sequence() {
            schema.required = req_array.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
    }

    // Parse closed
    if let Some(closed_val) = object_map.get(&serde_yaml::Value::String("closed".to_string())) {
        schema.closed = closed_val.as_bool().unwrap_or(false);
    }

    set_base_properties(map, &mut schema.annotations)?;

    Ok(Schema::Object(schema))
}

fn set_base_properties(
    map: &serde_yaml::Mapping,
    annotations: &mut SchemaAnnotations,
) -> Result<()> {
    // id
    if let Some(id_val) = map.get(&serde_yaml::Value::String("id".to_string())) {
        annotations.id = id_val.as_str().map(|s| s.to_string());
    }

    // description
    if let Some(desc_val) = map.get(&serde_yaml::Value::String("description".to_string())) {
        if let Some(desc_str) = desc_val.as_str() {
            annotations.description = Some(desc_str.to_string());
            annotations.documentation = Some(Documentation::Short(desc_str.to_string()));
        } else if let Some(desc_map) = desc_val.as_mapping() {
            let short = desc_map.get(&serde_yaml::Value::String("short".to_string()))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let long = desc_map.get(&serde_yaml::Value::String("long".to_string()))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if let (Some(short), Some(long)) = (short.clone(), long) {
                annotations.documentation = Some(Documentation::Long { short, long });
            } else if let Some(short) = short {
                annotations.documentation = Some(Documentation::Short(short));
            }
        }
    }

    // errorMessage
    if let Some(err_val) = map.get(&serde_yaml::Value::String("errorMessage".to_string())) {
        annotations.error_message = err_val.as_str().map(|s| s.to_string());
    }

    // hidden
    if let Some(hidden_val) = map.get(&serde_yaml::Value::String("hidden".to_string())) {
        annotations.hidden = hidden_val.as_bool().unwrap_or(false);
    }

    // completions
    if let Some(compl_val) = map.get(&serde_yaml::Value::String("completions".to_string())) {
        if let Some(compl_array) = compl_val.as_sequence() {
            annotations.completions = compl_array.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
    }

    // tags
    if let Some(tags_val) = map.get(&serde_yaml::Value::String("tags".to_string())) {
        if let Some(tags_map) = tags_val.as_mapping() {
            for (key, value) in tags_map {
                if let Some(key_str) = key.as_str() {
                    let json_val: serde_json::Value = serde_yaml::from_value(value.clone())?;
                    annotations.tags.insert(key_str.to_string(), json_val);
                }
            }
        }
    }

    Ok(())
}
```

### Public API

```rust
// ============================================================================
// PUBLIC API
// ============================================================================

pub struct Validator {
    schema_registry: SchemaRegistry,
}

impl Validator {
    pub fn new() -> Self {
        Self {
            schema_registry: SchemaRegistry::new(),
        }
    }

    pub fn register_schema(&mut self, schema: Schema) -> Result<()> {
        self.schema_registry.register(schema)
    }

    pub fn compile_schema_from_yaml(&mut self, yaml: &str) -> Result<Schema> {
        let schema = compile_schema_from_yaml(yaml)?;
        self.register_schema(schema.clone())?;
        Ok(schema)
    }

    pub fn compile_schemas_from_glob(&mut self, glob_pattern: &str) -> Result<Vec<Schema>> {
        let mut schemas = Vec::new();
        for path in glob::glob(glob_pattern)? {
            let path = path?;
            let content = std::fs::read_to_string(&path)?;
            let schema = self.compile_schema_from_yaml(&content)?;
            schemas.push(schema);
        }
        Ok(schemas)
    }

    pub fn validate(
        &self,
        value: &YamlWithSourceInfo,
        schema: &Schema,
        source: &MappedString,
    ) -> Result<Vec<LocalizedError>> {
        validate(value, schema, source, true)
    }

    pub fn validate_with_ref(
        &self,
        value: &YamlWithSourceInfo,
        schema_ref: &str,
        source: &MappedString,
    ) -> Result<Vec<LocalizedError>> {
        let schema = self.schema_registry.resolve(schema_ref)?;
        self.validate(value, schema, source)
    }
}

pub struct SchemaRegistry {
    schemas: HashMap<String, Schema>,
}

impl SchemaRegistry {
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
        }
    }

    pub fn register(&mut self, schema: Schema) -> Result<()> {
        let id = schema.id()
            .ok_or_else(|| anyhow!("Schema must have $id to be registered"))?;
        self.schemas.insert(id.to_string(), schema);
        Ok(())
    }

    pub fn resolve(&self, id: &str) -> Result<&Schema> {
        self.schemas.get(id)
            .ok_or_else(|| anyhow!("Schema '{}' not found in registry", id))
    }
}

impl Schema {
    pub fn id(&self) -> Option<&str> {
        match self {
            Schema::False | Schema::True => None,
            Schema::Boolean(s) => s.annotations.id.as_deref(),
            Schema::Number(s) => s.annotations.id.as_deref(),
            Schema::String(s) => s.annotations.id.as_deref(),
            Schema::Null(s) => s.annotations.id.as_deref(),
            Schema::Enum(s) => s.annotations.id.as_deref(),
            Schema::Any(s) => s.annotations.id.as_deref(),
            Schema::AnyOf(s) => s.annotations.id.as_deref(),
            Schema::AllOf(s) => s.annotations.id.as_deref(),
            Schema::Array(s) => s.annotations.id.as_deref(),
            Schema::Object(s) => s.annotations.id.as_deref(),
            Schema::Ref(s) => s.annotations.id.as_deref(),
        }
    }
}
```

## Usage Example

```rust
use quarto_yaml_validation::{Validator, Schema};
use quarto_yaml::{parse, YamlWithSourceInfo};

fn main() -> Result<()> {
    // Initialize validator
    let mut validator = Validator::new();

    // Compile schemas from YAML definitions
    validator.compile_schemas_from_glob("schemas/*.yml")?;

    // Parse YAML with source tracking
    let yaml_content = r#"
    ---
    title: "My Document"
    format:
      html:
        toc: true
        code-copy: hover
    ---
    "#;

    let (yaml_with_info, _) = parse(yaml_content)?;
    let source = MappedString::from_str(yaml_content);

    // Get document schema
    let schema = validator.resolve_schema("quarto-document-schema")?;

    // Validate
    let errors = validator.validate(&yaml_with_info, schema, &source)?;

    // Print errors
    for error in errors {
        println!("{}", error.nice_error.format());
    }

    Ok(())
}
```

## Implementation Phases

### Phase 1: Foundation (2 weeks)

**Deliverables**:
- Schema type definitions (`schema/types.rs`)
- Basic validation context (`validator/context.rs`)
- Navigate function (`validator/navigate.rs`)

**Tests**:
- Unit tests for each schema type
- Navigate function tests with complex nested structures

### Phase 2: Core Validators (2 weeks)

**Deliverables**:
- Type-specific validators (boolean, number, string, null, enum)
- Array and object validators
- AnyOf/AllOf combinators
- Basic error collection

**Tests**:
- Validation tests for each type
- Edge cases (empty arrays, nested objects, etc.)

### Phase 3: Schema Compilation (2 weeks)

**Deliverables**:
- YAML to Schema compiler (`schema/compiler.rs`)
- Schema registry with $ref resolution
- Support for all Quarto schema extensions (maybeArrayOf, closed, etc.)

**Tests**:
- Compile real Quarto schemas
- Round-trip tests (YAML → Schema → validate)

### Phase 4: Error Improvement (1-2 weeks)

**Deliverables**:
- Error handler infrastructure
- Typo suggestion (edit distance)
- YAML 1.0 boolean detection
- Bad colon/equals detection
- Error pruning for anyOf

**Tests**:
- Error message quality tests
- Compare with TypeScript output

### Phase 5: Integration (1 week)

**Deliverables**:
- Public API (`lib.rs`)
- Integration with quarto-yaml crate
- Example usage
- Documentation

**Tests**:
- End-to-end validation tests
- Performance benchmarks

### Phase 6: Polish (1 week)

**Deliverables**:
- Optimize performance
- Improve error messages
- Add remaining error handlers
- Update documentation

**Tests**:
- Stress tests with large schemas
- Comparison with TypeScript version

**Total: 6-8 weeks** (some parallelization possible)

## Open Questions

1. **Completions**: Should we implement IDE completion support in this crate, or defer to LSP?
   - Recommendation: Defer to LSP, focus on validation

2. **Schema format**: Should we support JSON Schema directly, or only YAML format?
   - Recommendation: Start with YAML only (Quarto's format), add JSON Schema later if needed

3. **Error handler extensibility**: Should third parties be able to register custom error handlers?
   - Recommendation: Yes, use trait-based design

4. **Caching**: Should compiled schemas be cached to disk?
   - Recommendation: Not initially, add later for performance

5. **Schema generation**: Should we support generating TypeScript/Zod/JSON Schema from YAML schemas?
   - Recommendation: Defer to separate tool, not core validation

## Dependencies

```toml
[dependencies]
quarto-yaml = { path = "../quarto-yaml" }  # YamlWithSourceInfo
quarto-errors = { path = "../quarto-errors" }  # TidyverseError
anyhow = "1.0"
thiserror = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_yaml = "0.9"
serde_json = "1.0"
regex = "1.10"
glob = "0.3"

[dev-dependencies]
pretty_assertions = "1.4"
insta = "1.40"  # Snapshot testing for error messages
```

## Comparison with JSON Schema Validators

### Why not use existing JSON Schema crates?

1. **jsonschema crate**: Full JSON Schema implementation
   - ❌ No source location tracking
   - ❌ Generic error messages
   - ❌ No Quarto extensions (maybeArrayOf, closed, etc.)
   - ❌ No error pruning for anyOf

2. **valico crate**: JSON Schema validator
   - ❌ Unmaintained (last update 2019)
   - ❌ No source tracking
   - ❌ No custom error messages

3. **Our custom validator**:
   - ✅ Tight integration with YamlWithSourceInfo
   - ✅ Excellent error messages with source context
   - ✅ Quarto-specific extensions
   - ✅ Error pruning and improvement
   - ✅ Complete control over error quality

**Verdict**: Build custom validator, leverage Quarto's proven design.

## Recommendations

### ✅ DO: Implement this design

**Rationale**:
1. Quarto's validation system is proven with excellent error messages
2. Tight integration with YamlWithSourceInfo is critical
3. Custom error handling is a key differentiator
4. Schema compilation from YAML is valuable

### ✅ DO: Start with core validation

**Phase 1-2 priorities**:
- Schema types and validation logic
- Navigate function (critical!)
- Basic error collection

### ✅ DO: Port error handlers incrementally

**Start with**:
- Type mismatch
- Typo suggestions
- YAML 1.0 booleans

**Defer**:
- Schema-defined errors
- Complex formatting

### ⚠️ DON'T: Implement completions initially

**Rationale**: LSP will handle IDE features. Focus on validation quality.

### ⚠️ DON'T: Support JSON Schema directly

**Rationale**: Quarto's YAML format is simpler and more maintainable. Can add JSON Schema support later if needed.

## Next Steps

1. Create `quarto-yaml-validation` crate structure
2. Implement Phase 1 (Schema types + Navigate)
3. Add comprehensive tests with Quarto schema fixtures
4. Implement Phase 2 (Core validators)
5. Port real Quarto schemas and validate output matches TypeScript

## References

- TypeScript validator: `quarto-cli/src/core/lib/yaml-validation/validator.ts`
- Schema types: `quarto-cli/src/core/lib/yaml-schema/types.ts`
- Error handling: `quarto-cli/src/core/lib/yaml-validation/errors.ts`
- Schema compiler: `quarto-cli/src/core/lib/yaml-schema/from-yaml.ts`
- Schema definitions: `quarto-cli/src/resources/schema/*.yml`
