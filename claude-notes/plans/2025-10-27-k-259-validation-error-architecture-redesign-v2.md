# k-259: Validation Error Architecture Redesign (v2)

**Date**: 2025-10-27 (Updated)
**Issue**: Redesign validation error architecture with wrapper type and filename-based locations
**Priority**: 0 (Critical)
**Parent**: k-253

---

## Executive Summary

Create a `ValidationDiagnostic` wrapper type that:
1. Preserves all structured validation data (paths, ranges)
2. Uses filenames instead of opaque file_id numbers
3. Wraps `DiagnosticMessage` for rendering infrastructure
4. Provides custom JSON output tailored for validation errors

**Key Decision**: Use a **wrapper type** instead of extending DiagnosticMessage with metadata.

---

## Architecture Design

### New Type: ValidationDiagnostic

```rust
/// A validation diagnostic with structured error information.
///
/// This type preserves all validation-specific structure (instance paths,
/// schema paths, source ranges) while delegating rendering to DiagnosticMessage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationDiagnostic {
    /// The validation error code (Q-1-xxx)
    pub code: String,

    /// Human-readable error message
    pub message: String,

    /// Path through the YAML instance where the error occurred
    /// Example: ["format", "html", "toc"]
    pub instance_path: Vec<PathSegment>,

    /// Path through the schema that was being validated
    /// Example: ["properties", "format", "properties", "html", "properties", "toc"]
    pub schema_path: Vec<String>,

    /// Source location with filename and byte offsets/line numbers
    pub source_range: Option<SourceRange>,

    /// Optional hints for fixing the error
    pub hints: Vec<String>,

    /// Internal: DiagnosticMessage for text rendering
    #[serde(skip)]
    diagnostic: DiagnosticMessage,
}

/// A segment in an instance path (object key or array index)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum PathSegment {
    /// Object property key
    Key(String),
    /// Array index
    Index(usize),
}

/// Source range with filename and both offset and line/column positions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRange {
    /// Filename (human-readable, not a file_id)
    pub filename: String,

    /// Start byte offset in the file
    pub start_offset: usize,

    /// End byte offset in the file
    pub end_offset: usize,

    /// Start line number (1-indexed)
    pub start_line: usize,

    /// Start column number (1-indexed)
    pub start_column: usize,

    /// End line number (1-indexed)
    pub end_line: usize,

    /// End column number (1-indexed)
    pub end_column: usize,
}
```

### Implementation

```rust
impl ValidationDiagnostic {
    /// Create a new ValidationDiagnostic from a ValidationError
    pub fn from_validation_error(
        error: &ValidationError,
        source_ctx: &SourceContext,
    ) -> Self {
        // Build the diagnostic message for text rendering
        let diagnostic = Self::build_diagnostic_message(error, source_ctx);

        // Extract source range with filename
        let source_range = error.yaml_node.as_ref().and_then(|node| {
            Self::extract_source_range(&node.source_info, source_ctx)
        });

        // Convert path segments
        let instance_path = error.instance_path.segments()
            .iter()
            .map(|seg| match seg {
                quarto_yaml_validation::PathSegment::Key(k) => PathSegment::Key(k.clone()),
                quarto_yaml_validation::PathSegment::Index(i) => PathSegment::Index(*i),
            })
            .collect();

        Self {
            code: infer_error_code(error),
            message: error.message.clone(),
            instance_path,
            schema_path: error.schema_path.segments().to_vec(),
            source_range,
            hints: suggest_fixes(error),
            diagnostic,
        }
    }

    /// Render as JSON for machine consumption
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;

        let mut obj = json!({
            "kind": "validation_error",
            "code": self.code,
            "message": self.message,
            "instance_path": self.instance_path,
            "schema_path": self.schema_path,
        });

        if let Some(range) = &self.source_range {
            obj["source_range"] = json!(range);
        }

        if !self.hints.is_empty() {
            obj["hints"] = json!(self.hints);
        }

        obj
    }

    /// Render as text for human consumption (uses ariadne/tidyverse)
    pub fn to_text(&self, source_ctx: &SourceContext) -> String {
        self.diagnostic.to_text(Some(source_ctx))
    }

    /// Helper: Build DiagnosticMessage for text rendering
    fn build_diagnostic_message(
        error: &ValidationError,
        source_ctx: &SourceContext,
    ) -> DiagnosticMessage {
        let mut builder = DiagnosticMessageBuilder::error("YAML Validation Failed")
            .with_code(infer_error_code(error))
            .problem(error.message.clone());

        // Attach full SourceInfo for ariadne rendering
        if let Some(yaml_node) = &error.yaml_node {
            builder = builder.with_location(yaml_node.source_info.clone());
        }

        // Add human-readable details
        if !error.instance_path.is_empty() {
            builder = builder.add_detail(format!(
                "At document path: `{}`",
                error.instance_path
            ));
        } else {
            builder = builder.add_detail("At document root");
        }

        if !error.schema_path.is_empty() {
            builder = builder.add_info(format!(
                "Schema constraint: {}",
                error.schema_path
            ));
        }

        // Add hints
        for hint in suggest_fixes(error) {
            builder = builder.add_hint(hint);
        }

        builder.build()
    }

    /// Helper: Extract SourceRange from SourceInfo
    fn extract_source_range(
        source_info: &SourceInfo,
        source_ctx: &SourceContext,
    ) -> Option<SourceRange> {
        // Map start offset
        let start_mapped = source_info.map_offset(source_info.start_offset(), source_ctx)?;

        // Map end offset
        let end_mapped = source_info.map_offset(source_info.end_offset(), source_ctx)?;

        // Get filename
        let file = source_ctx.get_file(start_mapped.file_id)?;

        Some(SourceRange {
            filename: file.path.clone(),
            start_offset: source_info.start_offset(),
            end_offset: source_info.end_offset(),
            start_line: start_mapped.location.row + 1,      // 1-indexed
            start_column: start_mapped.location.column + 1,  // 1-indexed
            end_line: end_mapped.location.row + 1,
            end_column: end_mapped.location.column + 1,
        })
    }
}
```

---

## Example JSON Output

```json
{
  "kind": "validation_error",
  "code": "Q-1-11",
  "message": "Expected number, got string",
  "instance_path": [
    {"type": "Key", "value": "year"}
  ],
  "schema_path": [
    "properties",
    "year",
    "number"
  ],
  "source_range": {
    "filename": "test.yaml",
    "start_offset": 18,
    "end_offset": 33,
    "start_line": 3,
    "start_column": 7,
    "end_line": 3,
    "end_column": 21
  },
  "hints": [
    "Use a numeric value without quotes?"
  ]
}
```

**Notice:**
- ✅ `filename` is a string, not a file_id
- ✅ Full range with start/end offsets AND line/column
- ✅ Structured `instance_path` array
- ✅ Structured `schema_path` array
- ✅ Clean, purpose-built JSON structure

---

## Example Text Output

```
Error [Q-1-11]: YAML Validation Failed
  ┌─ test.yaml:3:7
  │
3 │ year: "not a number"
  │       ^^^^^^^^^^^^^^ Expected number, got string
  │
Problem: Expected number, got string

  ✖ At document path: `year`
  ℹ Schema constraint: properties > year > number

  ? Use a numeric value without quotes?

See https://quarto.org/docs/errors/Q-1-11 for more information
```

---

## Comparison: Metadata vs Wrapper

### Approach A: Metadata Field (Original)

```rust
pub struct DiagnosticMessage {
    pub code: Option<String>,
    pub title: String,
    // ...
    pub metadata: Option<DiagnosticMetadata>,  // ← Generic catch-all
}

pub enum DiagnosticMetadata {
    YamlValidation { instance_path: Vec<...>, schema_path: Vec<...> },
    ParseError { ... },  // ← Enum grows with each subsystem
    TypeCheck { ... },
}
```

**Usage:**
```rust
if let Some(DiagnosticMetadata::YamlValidation { instance_path, .. }) = &diagnostic.metadata {
    // ❌ Need pattern matching everywhere
}
```

**Problems:**
- DiagnosticMessage becomes domain-aware (knows about YAML, parsing, type-checking)
- Metadata enum grows with every new subsystem
- Need Option and pattern matching
- JSON output has generic structure, less tailored

### Approach B: Wrapper Type (Recommended) ✅

```rust
pub struct ValidationDiagnostic {
    pub instance_path: Vec<PathSegment>,  // ← Always present, no Option
    pub schema_path: Vec<String>,
    pub source_range: SourceRange,
    diagnostic: DiagnosticMessage,  // ← Private, for rendering only
}
```

**Usage:**
```rust
let vd = ValidationDiagnostic::from_validation_error(&error, &ctx);
println!("{}", vd.instance_path[0]);  // ✅ Direct access, type-safe
```

**Benefits:**
- Clean separation: DiagnosticMessage stays generic, ValidationDiagnostic is specific
- Type-safe: No Option, no pattern matching
- Custom JSON tailored for validation errors
- Extensible without polluting DiagnosticMessage

---

## Changes Required

### 1. Add ValidationDiagnostic to quarto-yaml-validation (New)

**File**: `private-crates/quarto-yaml-validation/src/diagnostic.rs` (NEW)

Contains:
- `ValidationDiagnostic` struct
- `PathSegment` enum
- `SourceRange` struct
- Implementation of `from_validation_error()`, `to_json()`, `to_text()`

**Dependencies**:
- `quarto-error-reporting` (for DiagnosticMessage, DiagnosticMessageBuilder)
- `quarto-source-map` (for SourceContext, SourceInfo)
- `serde_json` (for JSON serialization)

### 2. Update validate-yaml to use ValidationDiagnostic

**File**: `private-crates/validate-yaml/src/main.rs`

```rust
// BEFORE:
match validate(&input_yaml, &schema, &registry, &source_ctx) {
    Err(error) => {
        let diagnostic = validation_error_to_diagnostic(&error);
        if args.json {
            println!("{}", diagnostic.to_json());
        } else {
            display_diagnostic(&diagnostic);
        }
    }
}

// AFTER:
match validate(&input_yaml, &schema, &registry, &source_ctx) {
    Err(error) => {
        let vd = ValidationDiagnostic::from_validation_error(&error, &source_ctx);
        if args.json {
            let output = json!({
                "success": false,
                "errors": [vd.to_json()]
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            eprintln!("{}", vd.to_text(&source_ctx));
        }
        process::exit(1);
    }
}
```

### 3. Remove error_conversion.rs (Deprecated)

The `error_conversion.rs` module is no longer needed. All conversion logic moves to `ValidationDiagnostic::from_validation_error()`.

### 4. Update quarto-error-reporting (Optional Enhancement)

No changes required to DiagnosticMessage itself, but we could add a helper:

```rust
impl DiagnosticMessage {
    /// Helper for domain-specific diagnostics to attach full SourceInfo
    pub fn with_full_source(mut self, source_info: SourceInfo) -> Self {
        self.location = Some(source_info);
        self
    }
}
```

---

## Implementation Plan

### Phase 1: Create ValidationDiagnostic (3-4 hours)

**Tasks**:
1. Create `private-crates/quarto-yaml-validation/src/diagnostic.rs`
2. Define `ValidationDiagnostic`, `PathSegment`, `SourceRange`
3. Implement `from_validation_error()`
4. Implement `to_json()` with custom structure
5. Implement `to_text()` by building DiagnosticMessage
6. Add unit tests for conversion and serialization

**Key implementation detail**: `extract_source_range()` must map both start and end offsets to get full range with line/column info.

### Phase 2: Update validate-yaml binary (1-2 hours)

**Tasks**:
1. Import `ValidationDiagnostic` from quarto-yaml-validation
2. Update error handling in `main.rs` to use `ValidationDiagnostic`
3. Remove `error_conversion.rs` module
4. Remove `error_codes.rs` (move to ValidationDiagnostic)
5. Test both JSON and text output

### Phase 3: Testing and Documentation (2-3 hours)

**Tasks**:
1. Add integration tests for JSON structure
2. Add integration tests for text output (ariadne)
3. Document JSON schema for downstream consumers
4. Add examples to documentation
5. Create TypeScript type definitions (optional)

**Total estimate**: 6-9 hours

---

## Benefits Summary

### 1. **Filenames Instead of file_id** ✅

**Before:**
```json
{"location": {"Original": {"file_id": 0, ...}}}
```

**After:**
```json
{"source_range": {"filename": "test.yaml", ...}}
```

Consumers don't need to maintain file registries!

### 2. **Full Source Ranges** ✅

Both offsets AND line/column for start and end:
```json
{
  "start_offset": 18,
  "end_offset": 33,
  "start_line": 3,
  "start_column": 7,
  "end_line": 3,
  "end_column": 21
}
```

Perfect for LSP diagnostics and editor highlighting!

### 3. **Structured Paths** ✅

```json
{
  "instance_path": [{"type": "Key", "value": "format"}, {"type": "Key", "value": "html"}],
  "schema_path": ["properties", "format", "properties", "html"]
}
```

Easy to parse and navigate programmatically!

### 4. **Type Safety** ✅

No `Option<Metadata>`, no pattern matching. If you have a `ValidationDiagnostic`, you have all the fields.

### 5. **Clean Separation** ✅

- `ValidationDiagnostic`: Domain-specific wrapper with rich structure
- `DiagnosticMessage`: Generic rendering infrastructure
- No pollution of generic types with validation specifics

---

## Testing Strategy

### JSON Output Tests

```rust
#[test]
fn test_validation_diagnostic_json_structure() {
    let error = ValidationError::new("Expected number, got string", /* ... */);
    let vd = ValidationDiagnostic::from_validation_error(&error, &source_ctx);

    let json = vd.to_json();

    // Check top-level fields
    assert_eq!(json["kind"], "validation_error");
    assert_eq!(json["code"], "Q-1-11");
    assert_eq!(json["message"], "Expected number, got string");

    // Check instance_path is structured array
    assert!(json["instance_path"].is_array());
    assert_eq!(json["instance_path"][0]["type"], "Key");
    assert_eq!(json["instance_path"][0]["value"], "year");

    // Check schema_path is string array
    assert!(json["schema_path"].is_array());
    assert_eq!(json["schema_path"][0], "properties");

    // Check source_range has filename (not file_id!)
    assert_eq!(json["source_range"]["filename"], "test.yaml");
    assert!(json["source_range"]["start_offset"].is_number());
    assert!(json["source_range"]["start_line"].is_number());
}
```

### Text Output Tests

```rust
#[test]
fn test_validation_diagnostic_text_has_ariadne() {
    let error = ValidationError::new("Expected number, got string", /* ... */);
    let vd = ValidationDiagnostic::from_validation_error(&error, &source_ctx);

    let text = vd.to_text(&source_ctx);

    // Should have box-drawing from ariadne
    assert!(text.contains("┌─") || text.contains("│"));

    // Should have filename and line number
    assert!(text.contains("test.yaml:3:7"));

    // Should have error code
    assert!(text.contains("[Q-1-11]"));
}
```

---

## Open Questions

### Q1: Should SourceRange include the source text snippet?

**Option 1**: No (current proposal)
- Consumers read file themselves
- Keeps JSON output smaller

**Option 2**: Yes, include snippet
```json
{
  "source_range": {
    "filename": "test.yaml",
    "start_line": 3,
    "text": "year: \"not a number\""
  }
}
```

**Recommendation**: No for now. Add later if needed via `--include-source` flag.

### Q2: Should we version the JSON output?

**Recommendation**: Yes, add version field:
```json
{
  "version": "1.0",
  "success": false,
  "errors": [...]
}
```

### Q3: Where should ValidationDiagnostic live?

**Options**:
1. In `quarto-yaml-validation` (alongside ValidationError)
2. In `validate-yaml` (binary-specific)
3. In `quarto-error-reporting` (generic infrastructure)

**Recommendation**: Option 1 (quarto-yaml-validation). It's validation-specific and should live with ValidationError.

---

## Migration Path

### Step 1: Add ValidationDiagnostic

Add new types without breaking existing code. `ValidationError` continues to exist and work.

### Step 2: Update validate-yaml

Binary switches to using `ValidationDiagnostic`. Old `error_conversion.rs` can be removed.

### Step 3: Document JSON schema

Publish JSON schema and TypeScript types for downstream consumers.

### Step 4: Future - Public API

If other crates need ValidationDiagnostic, it's already public in quarto-yaml-validation.

---

## Success Criteria

### Must Have ✅

- [ ] ValidationDiagnostic created with all required fields
- [ ] JSON output has structured instance_path and schema_path arrays
- [ ] JSON output uses filename strings, not file_id numbers
- [ ] JSON output includes full source range (offsets + line/column)
- [ ] Text output uses ariadne for visual highlighting
- [ ] All existing tests pass

### Should Have 🟡

- [ ] Integration tests for JSON structure
- [ ] Integration tests for text output
- [ ] Documentation with examples
- [ ] JSON schema documented

### Nice to Have 🟢

- [ ] TypeScript type definitions
- [ ] Python type definitions
- [ ] Example downstream consumer
- [ ] Performance benchmarks

---

## Timeline

| Phase | Description | Hours |
|-------|-------------|-------|
| 1 | Create ValidationDiagnostic | 3-4 |
| 2 | Update validate-yaml | 1-2 |
| 3 | Testing and docs | 2-3 |
| **Total** | | **6-9** |

---

## Conclusion

**Approach**: Create `ValidationDiagnostic` wrapper type

**Key improvements**:
1. ✅ Filenames instead of file_id numbers
2. ✅ Wrapper type instead of metadata field
3. ✅ Custom JSON tailored for validation
4. ✅ Full source ranges with line/column
5. ✅ Type-safe access to structured data

**Next step**: Implement Phase 1 (create ValidationDiagnostic)
