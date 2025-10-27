# Plan: Machine-Readable Validation Errors

## Problem Statement

Currently, `ValidationDiagnostic` converts structured `ValidationErrorKind` data into strings early (at construction time), losing machine-readable information:

```rust
// Current - lossy conversion
Self {
    code: error.error_code().to_string(),
    message: error.message(),  // ← Structured data → string
    hints: Self::suggest_fixes(error),  // ← Generated from kind
    ...
}
```

JSON output looks like:
```json
{
  "kind": "validation_error",
  "code": "Q-1-11",
  "message": "Expected number, got string"  // ← Lost: which type? what value?
}
```

A machine consumer can't extract:
- What type was expected? (need to parse "Expected number")
- What type was received? (need to parse "got string")
- The actual value that failed
- Constraint values (min/max, pattern, etc.)

## Principle

**Data should be stored in machine-preferred formats for as long as possible, and only converted to human-preferred formats like strings when needed.**

## Current Architecture Issues

### Issue 1: Early String Conversion in ValidationDiagnostic

`ValidationDiagnostic` (line 130-138) immediately converts:
- `ValidationErrorKind` → `String` via `.message()`
- Hints derived from kind, stored as `Vec<String>`

This loses all structured data before JSON serialization.

### Issue 2: No Serde Implementation for ValidationErrorKind

`ValidationErrorKind` is marked `#[derive(Debug, Clone, PartialEq)]` but NOT `Serialize/Deserialize`.

Without serde derives, we can't serialize the enum directly to JSON.

### Issue 3: ValidationDiagnostic Has Dual Purpose

ValidationDiagnostic tries to serve two masters:
1. **Machine-readable**: JSON output with structured data
2. **Human-readable**: Text output with formatted strings

This creates tension - we want structured data for JSON, but we've already converted to strings.

## Proposed Solution

### High-Level Approach

**Option A: Keep ValidationErrorKind in ValidationDiagnostic**
- Add `#[derive(Serialize, Deserialize)]` to `ValidationErrorKind`
- Store `kind: ValidationErrorKind` in `ValidationDiagnostic` (alongside or replacing `message`)
- Generate `message` lazily when needed for text output
- Serialize `kind` directly in JSON output

**Option B: Make ValidationDiagnostic a thin wrapper**
- Store full `ValidationError` in `ValidationDiagnostic`
- Generate all display strings lazily
- JSON serialization directly accesses `error.kind`

**Recommendation: Option A** - cleaner separation, ValidationDiagnostic becomes pure data

### Detailed Plan for Option A

#### Step 1: Add Serde to ValidationErrorKind

```rust
// error.rs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ValidationErrorKind {
    SchemaFalse,
    TypeMismatch {
        expected: String,
        got: String,
    },
    MissingRequiredProperty {
        property: String,
    },
    // ... rest
}
```

**Output example:**
```json
{
  "type": "TypeMismatch",
  "data": {
    "expected": "number",
    "got": "string"
  }
}
```

#### Step 2: Update ValidationDiagnostic Structure

```rust
// diagnostic.rs
#[derive(Debug, Clone)]
pub struct ValidationDiagnostic {
    /// Structured error kind - machine readable
    pub kind: ValidationErrorKind,

    /// Error code (derived from kind)
    pub code: String,

    /// Path through the YAML instance
    pub instance_path: Vec<PathSegment>,

    /// Path through the schema
    pub schema_path: Vec<String>,

    /// Source location
    pub source_range: Option<SourceRange>,

    /// Internal: DiagnosticMessage for text rendering
    diagnostic: DiagnosticMessage,
}
```

**Remove:**
- `message: String` field (derive from `kind` when needed)
- `hints: Vec<String>` field (derive from `kind` when needed)

#### Step 3: Update from_validation_error

```rust
pub fn from_validation_error(
    error: &ValidationError,
    source_ctx: &SourceContext,
) -> Self {
    let diagnostic = Self::build_diagnostic_message(error, source_ctx);
    let source_range = error.yaml_node.as_ref()
        .and_then(|node| Self::extract_source_range(&node.source_info, source_ctx));

    let instance_path = error.instance_path.segments()
        .iter()
        .map(|seg| match seg {
            crate::error::PathSegment::Key(k) => PathSegment::Key(k.clone()),
            crate::error::PathSegment::Index(i) => PathSegment::Index(*i),
        })
        .collect();

    Self {
        kind: error.kind.clone(),  // ← Store structured data
        code: error.error_code().to_string(),
        instance_path,
        schema_path: error.schema_path.segments().to_vec(),
        source_range,
        diagnostic,
    }
}
```

#### Step 4: Update to_json for Machine-Readable Output

```rust
pub fn to_json(&self) -> serde_json::Value {
    use serde_json::json;

    let mut obj = json!({
        "error_kind": self.kind,  // ← Structured, machine-readable
        "code": self.code,
        "instance_path": self.instance_path,
        "schema_path": self.schema_path,
    });

    if let Some(range) = &self.source_range {
        obj["source_range"] = json!(range);
    }

    // Optional: include human-readable fields for convenience
    obj["message"] = json!(self.kind.message());
    obj["hints"] = json!(Self::suggest_fixes_from_kind(&self.kind));

    obj
}
```

**Result:**
```json
{
  "error_kind": {
    "type": "TypeMismatch",
    "data": {
      "expected": "number",
      "got": "string"
    }
  },
  "code": "Q-1-11",
  "instance_path": [{"type": "Key", "value": "age"}],
  "schema_path": ["object", "number"],
  "source_range": { ... },
  "message": "Expected number, got string",
  "hints": ["Use a numeric value without quotes?"]
}
```

#### Step 5: Add Accessor Methods for Lazy String Generation

```rust
impl ValidationDiagnostic {
    /// Get human-readable message (lazily generated)
    pub fn message(&self) -> String {
        self.kind.message()
    }

    /// Get hints (lazily generated)
    pub fn hints(&self) -> Vec<String> {
        Self::suggest_fixes_from_kind(&self.kind)
    }

    // Rename suggest_fixes to suggest_fixes_from_kind
    fn suggest_fixes_from_kind(kind: &ValidationErrorKind) -> Vec<String> {
        // ... existing logic
    }
}
```

#### Step 6: Update suggest_fixes to take &ValidationErrorKind

Currently `suggest_fixes` takes `&ValidationError`, change to:

```rust
fn suggest_fixes_from_kind(kind: &ValidationErrorKind) -> Vec<String> {
    match kind {
        ValidationErrorKind::MissingRequiredProperty { property } => {
            vec![format!("Add the `{}` property to your YAML document?", property)]
        }
        ValidationErrorKind::TypeMismatch { expected, .. } => {
            match expected.as_str() {
                "boolean" => vec!["Use `true` or `false` (YAML 1.2 standard)?".to_string()],
                // ... rest
            }
        }
        // ... rest
    }
}
```

## Benefits

### For Machine Consumers

JSON output is now fully structured:
```json
{
  "error_kind": {
    "type": "NumberOutOfRange",
    "data": {
      "value": 150,
      "minimum": null,
      "maximum": 100,
      "exclusive_minimum": null,
      "exclusive_maximum": null
    }
  }
}
```

Consumers can:
- Match on error type programmatically
- Extract exact constraint values
- Build custom error messages in any language
- Implement auto-fix logic based on structured data

### For Human Consumers

No change - text output still uses ariadne rendering via `diagnostic.to_text()`.

Optional: JSON can still include `message` and `hints` fields for convenience.

## Implementation Steps

1. **Phase 1**: Add serde derives to ValidationErrorKind
   - Add dependency: `serde = { workspace = true, features = ["derive"] }`
   - Add derives to ValidationErrorKind
   - Write serialization tests

2. **Phase 2**: Update ValidationDiagnostic structure
   - Add `kind: ValidationErrorKind` field
   - Remove `message: String` field
   - Remove `hints: Vec<String>` field
   - Add `message()` and `hints()` accessor methods

3. **Phase 3**: Update from_validation_error
   - Store `error.kind.clone()` instead of calling `.message()`
   - Remove `suggest_fixes` call from constructor

4. **Phase 4**: Update to_json
   - Serialize `error_kind` with structured data
   - Optionally include `message` and `hints` for convenience

5. **Phase 5**: Update suggest_fixes
   - Rename to `suggest_fixes_from_kind`
   - Change parameter from `&ValidationError` to `&ValidationErrorKind`

6. **Phase 6**: Testing
   - Update all tests
   - Verify JSON output has structured data
   - Verify text output unchanged
   - Test round-trip serialization

## Migration Notes

- This is a breaking change to JSON output format
- Old: `{"kind": "validation_error", "message": "..."}`
- New: `{"error_kind": {"type": "TypeMismatch", ...}, "message": "..."}`
- The `message` field can remain for backward compatibility
- Consumers should migrate to use `error_kind` for structured data

## Alternative Considered

**Option B: Store full ValidationError**

```rust
pub struct ValidationDiagnostic {
    error: ValidationError,
    source_range: Option<SourceRange>,
    diagnostic: DiagnosticMessage,
}
```

**Pros:**
- Even simpler structure
- No duplication of paths

**Cons:**
- ValidationError isn't Serialize/Deserialize
- Contains SourceInfo which has complex nested structure
- Mixing concerns (validation logic with diagnostic presentation)

**Decision:** Option A is cleaner separation of concerns.

## Related Issues

- Might want to consider similar approach for other error types in the codebase
- Consider if ValidationError itself should be Serialize/Deserialize
- Consider if we want a formal JSON Schema for the error format

## Questions for User

1. Should we keep `message` and `hints` in JSON for convenience, or make them opt-in?
2. Do we need backward compatibility with current JSON format?
3. Should we version the JSON output format?
4. Do we want to support deserialization (JSON → ValidationErrorKind)?
