# k-259: Validation Error Architecture Redesign

**Date**: 2025-10-27
**Issue**: Redesign validation error architecture to separate data from presentation
**Priority**: 0 (Critical)
**Parent**: k-253 (YAML validation error reporting improvements)

---

## Executive Summary

The current validation error architecture loses machine-readable structure when converting to `DiagnosticMessage`. We need to redesign the error flow to preserve structured paths, full source ranges, and enable both human-friendly (ariadne/text) and machine-friendly (JSON) output from the same data structure.

**Key Changes Needed**:
1. Preserve full `SourceInfo` (with start/end offsets) instead of single line/column
2. Add structured instance path and schema path to JSON output
3. Separate error data from error presentation following quarto-markdown-pandoc pattern
4. Enable rich JSON output for downstream tooling (LSP, CI/CD, editors)

---

## Current State Analysis

### Architecture Flow (Current)

```
ValidationError (quarto-yaml-validation)
  ├─ message: String
  ├─ instance_path: InstancePath          ← Array of PathSegment (Key | Index)
  ├─ schema_path: SchemaPath              ← Array of String
  ├─ yaml_node: Option<YamlWithSourceInfo> ← Has full SourceInfo!
  └─ location: Option<SourceLocation>     ← Only line/column (loses range!)
         ↓
    CONVERSION (error_conversion.rs)
         ↓
DiagnosticMessage (quarto-error-reporting)
  ├─ title: String
  ├─ problem: Option<MessageContent>
  ├─ details: Vec<DetailItem>             ← Paths converted to STRINGS
  │   └─ content: MessageContent::Markdown("At document path: `format.html`")
  ├─ hints: Vec<MessageContent>
  └─ location: Option<SourceInfo>         ← NOT SET from ValidationError!
         ↓
    to_json() / to_text()
         ↓
Output (JSON or Text)
```

### Problems with Current Architecture

#### Problem 1: Loss of Structured Paths ❌

**Current JSON output:**
```json
{
  "details": [
    {
      "kind": "error",
      "content": {
        "type": "markdown",
        "content": "At document path: `format.html.toc`"  ← STRING, not array!
      }
    }
  ]
}
```

**Desired JSON output:**
```json
{
  "instance_path": ["format", "html", "toc"],  ← Structured array
  "schema_path": ["properties", "format", "properties", "html", "properties", "toc"]
}
```

**Why it matters**: Downstream tools (LSP servers, linters, editors) need structured paths to:
- Navigate to exact document location
- Build error trees
- Group errors by path
- Offer quick fixes

#### Problem 2: Loss of Source Range ❌

**Current:**
- `ValidationError` has `yaml_node: Option<YamlWithSourceInfo>`
  - Contains `SourceInfo` with **full range** (start_offset, end_offset)
- But `with_yaml_node()` only extracts **single point** (line, column)
- `location: Option<SourceLocation>` only stores line/column, no range

**Example:**
```yaml
year: "not a number"  # Error spans offsets 6-20
```

**Current output:**
```
In file `test.yaml` at line 3, column 7  ← Single point only
```

**Desired output (JSON)**:
```json
{
  "location": {
    "Original": {
      "file_id": 0,
      "start_offset": 6,
      "end_offset": 20
    }
  }
}
```

**Why it matters**:
- ariadne needs full range to draw nice squiggly underlines
- LSP needs ranges for diagnostics
- Editors need ranges for highlighting

#### Problem 3: SourceInfo Not Attached to DiagnosticMessage ❌

**Current code** (error_conversion.rs:16):
```rust
pub fn validation_error_to_diagnostic(error: &ValidationError) -> DiagnosticMessage {
    let mut builder = DiagnosticMessageBuilder::error("YAML Validation Failed")
        .with_code(infer_error_code(error))
        .problem(error.message.clone());

    // ...

    // ❌ NO .with_location() call!
    // The yaml_node SourceInfo is never attached to the DiagnosticMessage

    builder.build()
}
```

**Result**: `DiagnosticMessage.location` is always `None`, so:
- ariadne visual output doesn't work
- JSON output has no location field
- All source tracking is lost

#### Problem 4: Conversion Happens Too Early ❌

**Current pattern:**
```rust
// validate-yaml/src/main.rs
match validate(&input_yaml, &schema, &registry, &source_ctx) {
    Err(error) => {
        // ❌ Convert IMMEDIATELY, losing structure
        let diagnostic = validation_error_to_diagnostic(&error);

        if args.json {
            println!("{}", diagnostic.to_json());  // Already lost paths!
        }
    }
}
```

**Problem**: Once converted to DiagnosticMessage, structured paths are gone.

---

## Comparison with quarto-markdown-pandoc

### How quarto-markdown-pandoc Does It ✅

```rust
// 1. Error creation preserves ALL structure
let error = DiagnosticMessageBuilder::error("Parse error")
    .with_code("Q-2-1")
    .with_location(source_info)  // ← Full SourceInfo preserved
    .problem("Unclosed code block")
    .add_detail_at("Started here", start_loc)
    .add_detail_at("No closing fence found", end_loc)
    .build();

// 2. DiagnosticMessage has location: Option<SourceInfo>
//    SourceInfo is an enum with full offset ranges:
//    - Original { file_id, start_offset, end_offset }
//    - Substring { parent, start_offset, end_offset }

// 3. Rendering happens late with full structure
if args.json {
    println!("{}", error.to_json());  // Serializes SourceInfo directly
} else {
    eprintln!("{}", error.to_text(Some(&source_ctx)));  // Uses ariadne
}
```

**Key differences:**
1. **SourceInfo preserved**: Location stored as `SourceInfo` enum (has full ranges)
2. **Structured from start**: Errors built with all information
3. **Deferred rendering**: JSON/text decision made at output time
4. **Serde integration**: `SourceInfo` derives `Serialize`, outputs structured JSON

### Example JSON Output from quarto-markdown-pandoc

```json
{
  "kind": "error",
  "title": "Unclosed code block",
  "code": "Q-2-1",
  "problem": {
    "type": "markdown",
    "content": "Code block started but never closed"
  },
  "location": {
    "Original": {
      "file_id": 0,
      "start_offset": 0,
      "end_offset": 11
    }
  },
  "details": [
    {
      "kind": "info",
      "content": {
        "type": "markdown",
        "content": "Started here"
      },
      "location": {
        "Original": {
          "file_id": 0,
          "start_offset": 0,
          "end_offset": 3
        }
      }
    }
  ]
}
```

**Notice:**
- `location` is a structured object (not a string!)
- Has `file_id`, `start_offset`, `end_offset`
- Can be deserialized and used programmatically

---

## Proposed Solution

### Design Principles

1. **Separate data from presentation** - Store all structured information
2. **Late rendering** - Convert to text/JSON only at output time
3. **Rich JSON output** - Preserve paths and locations as structured data
4. **Backward compatibility** - Text output should remain similar
5. **Reuse existing infrastructure** - Leverage `SourceInfo` serde support

### Approach A: Extend DiagnosticMessage (Recommended)

Add optional metadata fields to `DiagnosticMessage` for validation-specific structure.

#### Changes to quarto-error-reporting

**File**: `crates/quarto-error-reporting/src/diagnostic.rs`

```rust
/// A diagnostic message following tidyverse error message guidelines
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticMessage {
    pub code: Option<String>,
    pub title: String,
    pub kind: DiagnosticKind,
    pub problem: Option<MessageContent>,
    pub details: Vec<DetailItem>,
    pub hints: Vec<MessageContent>,
    pub location: Option<SourceInfo>,

    // NEW: Optional structured metadata for domain-specific errors
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<DiagnosticMetadata>,
}

/// Domain-specific metadata for diagnostic messages
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DiagnosticMetadata {
    /// YAML validation metadata
    YamlValidation {
        /// Path through the instance (document) where error occurred
        instance_path: Vec<PathSegment>,
        /// Path through the schema that was being validated
        schema_path: Vec<String>,
    },
    // Future: Other domain-specific metadata types
    // ParseError { ... },
    // TypeCheck { ... },
}

/// Segment in an instance path
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum PathSegment {
    /// Object property key
    Key(String),
    /// Array index
    Index(usize),
}
```

**Update `to_json()` method:**

```rust
impl DiagnosticMessage {
    pub fn to_json(&self) -> serde_json::Value {
        // ... existing code ...

        if let Some(location) = &self.location {
            obj["location"] = json!(location);  // Already works!
        }

        // NEW: Serialize metadata if present
        if let Some(metadata) = &self.metadata {
            obj["metadata"] = json!(metadata);
        }

        obj
    }
}
```

**JSON output example:**
```json
{
  "kind": "error",
  "title": "YAML Validation Failed",
  "code": "Q-1-11",
  "problem": {
    "type": "markdown",
    "content": "Expected number, got string"
  },
  "location": {
    "Original": {
      "file_id": 0,
      "start_offset": 18,
      "end_offset": 33
    }
  },
  "metadata": {
    "type": "YamlValidation",
    "instance_path": [
      {"type": "Key", "value": "year"}
    ],
    "schema_path": [
      "properties",
      "year",
      "number"
    ]
  }
}
```

#### Changes to quarto-yaml-validation

**File**: `private-crates/quarto-yaml-validation/src/error.rs`

NO CHANGES NEEDED! Keep ValidationError as-is.

#### Changes to validate-yaml

**File**: `private-crates/validate-yaml/src/error_conversion.rs`

```rust
use quarto_error_reporting::{
    DiagnosticMessage, DiagnosticMessageBuilder, DiagnosticMetadata, PathSegment,
};
use quarto_yaml_validation::ValidationError;

pub fn validation_error_to_diagnostic(error: &ValidationError) -> DiagnosticMessage {
    let mut builder = DiagnosticMessageBuilder::error("YAML Validation Failed")
        .with_code(infer_error_code(error))
        .problem(error.message.clone());

    // NEW: Attach full SourceInfo from yaml_node
    if let Some(yaml_node) = &error.yaml_node {
        builder = builder.with_location(yaml_node.source_info.clone());
    }

    // Add human-readable details (for text output)
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
    if let Some(hint) = suggest_fix(error) {
        builder = builder.add_hint(hint);
    }

    // NEW: Build and attach metadata
    let mut diagnostic = builder.build();

    diagnostic.metadata = Some(DiagnosticMetadata::YamlValidation {
        instance_path: error.instance_path.segments()
            .iter()
            .map(|seg| match seg {
                quarto_yaml_validation::PathSegment::Key(k) => {
                    PathSegment::Key(k.clone())
                }
                quarto_yaml_validation::PathSegment::Index(i) => {
                    PathSegment::Index(*i)
                }
            })
            .collect(),
        schema_path: error.schema_path.segments().to_vec(),
    });

    diagnostic
}
```

**Benefits:**
- ✅ Text output unchanged (still uses .add_detail() strings)
- ✅ JSON output gains structured paths in metadata field
- ✅ Full SourceInfo preserved in location field
- ✅ Backward compatible with existing DiagnosticMessage consumers

---

### Approach B: Direct JSON Serialization (Alternative)

Add `to_json()` and `to_text()` methods directly to `ValidationError`.

**File**: `private-crates/quarto-yaml-validation/src/error.rs`

```rust
impl ValidationError {
    /// Render this validation error as JSON
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;

        let mut obj = json!({
            "kind": "error",
            "message": self.message,
            "instance_path": self.instance_path.segments()
                .iter()
                .map(|seg| match seg {
                    PathSegment::Key(k) => json!({"type": "key", "value": k}),
                    PathSegment::Index(i) => json!({"type": "index", "value": i}),
                })
                .collect::<Vec<_>>(),
            "schema_path": self.schema_path.segments(),
        });

        if let Some(node) = &self.yaml_node {
            obj["location"] = json!(node.source_info);
        }

        obj
    }

    /// Render this validation error as text (tidyverse style)
    pub fn to_text(&self, ctx: &SourceContext) -> String {
        // Convert to DiagnosticMessage and use its to_text()
        let diagnostic = validation_error_to_diagnostic(self);
        diagnostic.to_text(Some(ctx))
    }
}
```

**Benefits:**
- ✅ Keep validation errors self-contained
- ✅ Direct control over JSON structure
- ✅ Simple implementation

**Drawbacks:**
- ❌ Duplicates rendering logic
- ❌ Doesn't leverage DiagnosticMessage infrastructure (ariadne, tidyverse formatting)
- ❌ Inconsistent with quarto-markdown-pandoc pattern

**Verdict**: Approach A is better for consistency.

---

### Approach C: Create ValidationDiagnostic Wrapper (Overengineered)

Create a new `ValidationDiagnostic` that wraps both `ValidationError` and `DiagnosticMessage`.

**Not recommended** - adds unnecessary complexity.

---

## Recommended Implementation Plan

### Phase 1: Extend DiagnosticMessage with Metadata (2-3 hours)

**Goal**: Add optional metadata field to preserve structured information.

**Tasks**:
1. Add `DiagnosticMetadata` enum to `crates/quarto-error-reporting/src/diagnostic.rs`
2. Add `PathSegment` enum for instance paths
3. Add `metadata: Option<DiagnosticMetadata>` field to `DiagnosticMessage`
4. Update `to_json()` to serialize metadata
5. Add unit tests for metadata serialization

**Files**:
- `crates/quarto-error-reporting/src/diagnostic.rs`

**Testing**:
```rust
#[test]
fn test_metadata_serialization() {
    let diagnostic = DiagnosticMessage {
        // ...
        metadata: Some(DiagnosticMetadata::YamlValidation {
            instance_path: vec![
                PathSegment::Key("format".into()),
                PathSegment::Key("html".into()),
            ],
            schema_path: vec!["properties".into(), "format".into()],
        }),
    };

    let json = diagnostic.to_json();
    assert!(json["metadata"]["instance_path"].is_array());
}
```

---

### Phase 2: Update error_conversion.rs (1-2 hours)

**Goal**: Attach full SourceInfo and metadata when converting ValidationError.

**Tasks**:
1. Extract SourceInfo from `yaml_node` and attach via `.with_location()`
2. Build metadata from instance_path and schema_path
3. Attach metadata to diagnostic after building
4. Test with validate-yaml

**Files**:
- `private-crates/validate-yaml/src/error_conversion.rs`

**Testing**:
```bash
# Should now output location with full range
validate-yaml --input test.yaml --schema schema.yaml --json | jq .errors[0].location
{
  "Original": {
    "file_id": 0,
    "start_offset": 18,
    "end_offset": 33
  }
}

# Should output structured paths
validate-yaml --input test.yaml --schema schema.yaml --json | jq .errors[0].metadata.instance_path
[
  {"type": "Key", "value": "year"}
]
```

---

### Phase 3: Enable ariadne Visual Output (1-2 hours)

**Goal**: Use full SourceInfo for beautiful source highlighting.

**Current**: Text output doesn't use ariadne because location is None.

**After Phase 2**: location will have SourceInfo, so ariadne should work automatically.

**Tasks**:
1. Test that ariadne rendering works in text mode
2. Adjust display_diagnostic() if needed
3. Add test cases for visual output

**Expected output**:
```
Error [Q-1-11]: YAML Validation Failed
  ┌─ invalid-document.yaml:3:7
  │
3 │ year: "not a number"
  │       ^^^^^^^^^^^^^^ Expected number, got string
  │
  = Use a numeric value without quotes?
```

---

### Phase 4: Add Helper Method for JSON Path Resolution (Optional, 1 hour)

**Goal**: Make it easy for downstream tools to resolve paths.

**Add to DiagnosticMessage**:
```rust
impl DiagnosticMessage {
    /// Get structured instance path if this is a validation error
    pub fn instance_path(&self) -> Option<&[PathSegment]> {
        match &self.metadata {
            Some(DiagnosticMetadata::YamlValidation { instance_path, .. }) => {
                Some(instance_path)
            }
            _ => None,
        }
    }

    /// Get structured schema path if this is a validation error
    pub fn schema_path(&self) -> Option<&[String]> {
        match &self.metadata {
            Some(DiagnosticMetadata::YamlValidation { schema_path, .. }) => {
                Some(schema_path)
            }
            _ => None,
        }
    }
}
```

---

## Example Outputs After Implementation

### Text Output (Human-Friendly)

**Command**: `validate-yaml --input test.yaml --schema schema.yaml`

**Output**:
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

### JSON Output (Machine-Friendly)

**Command**: `validate-yaml --input test.yaml --schema schema.yaml --json`

**Output**:
```json
{
  "success": false,
  "errors": [
    {
      "kind": "error",
      "title": "YAML Validation Failed",
      "code": "Q-1-11",
      "problem": {
        "type": "markdown",
        "content": "Expected number, got string"
      },
      "details": [
        {
          "kind": "error",
          "content": {
            "type": "markdown",
            "content": "At document path: `year`"
          }
        },
        {
          "kind": "info",
          "content": {
            "type": "markdown",
            "content": "Schema constraint: properties > year > number"
          }
        }
      ],
      "hints": [
        {
          "type": "markdown",
          "content": "Use a numeric value without quotes?"
        }
      ],
      "location": {
        "Original": {
          "file_id": 0,
          "start_offset": 18,
          "end_offset": 33
        }
      },
      "metadata": {
        "type": "YamlValidation",
        "instance_path": [
          {"type": "Key", "value": "year"}
        ],
        "schema_path": [
          "properties",
          "year",
          "number"
        ]
      }
    }
  ]
}
```

**Downstream tools can now**:
- Parse `metadata.instance_path` to navigate document structure
- Parse `metadata.schema_path` to understand which schema constraint failed
- Use `location.start_offset` and `location.end_offset` for highlighting
- Map `file_id` to actual filename using separate file registry

---

## Testing Strategy

### Unit Tests

**File**: `crates/quarto-error-reporting/src/diagnostic.rs`

```rust
#[test]
fn test_yaml_validation_metadata() {
    let diagnostic = DiagnosticMessage {
        kind: DiagnosticKind::Error,
        title: "Validation failed".to_string(),
        code: Some("Q-1-11".into()),
        problem: None,
        details: vec![],
        hints: vec![],
        location: Some(SourceInfo::original(FileId(0), 10, 20)),
        metadata: Some(DiagnosticMetadata::YamlValidation {
            instance_path: vec![PathSegment::Key("year".into())],
            schema_path: vec!["properties".into(), "year".into()],
        }),
    };

    let json = diagnostic.to_json();

    // Check metadata is present
    assert!(json["metadata"].is_object());
    assert_eq!(json["metadata"]["type"], "YamlValidation");

    // Check instance_path
    let path = &json["metadata"]["instance_path"];
    assert!(path.is_array());
    assert_eq!(path[0]["type"], "Key");
    assert_eq!(path[0]["value"], "year");

    // Check schema_path
    let schema_path = &json["metadata"]["schema_path"];
    assert!(schema_path.is_array());
    assert_eq!(schema_path[0], "properties");
}
```

### Integration Tests

**File**: `private-crates/validate-yaml/tests/json_output_tests.rs`

```rust
#[test]
fn test_json_output_has_structured_paths() {
    let output = Command::new("validate-yaml")
        .args(&["--input", "test-data/type-mismatch.yaml"])
        .args(&["--schema", "test-data/schema.yaml"])
        .arg("--json")
        .output()
        .expect("Failed to run validate-yaml");

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("Invalid JSON output");

    // Check error has metadata
    let error = &json["errors"][0];
    assert!(error["metadata"].is_object());

    // Check instance_path is structured array
    let instance_path = &error["metadata"]["instance_path"];
    assert!(instance_path.is_array());
    assert!(instance_path[0].is_object());

    // Check location has full range
    let location = &error["location"];
    assert!(location["Original"]["start_offset"].is_number());
    assert!(location["Original"]["end_offset"].is_number());
}

#[test]
fn test_text_output_has_ariadne() {
    let output = Command::new("validate-yaml")
        .args(&["--input", "test-data/type-mismatch.yaml"])
        .args(&["--schema", "test-data/schema.yaml"])
        .output()
        .expect("Failed to run validate-yaml");

    let text = String::from_utf8_lossy(&output.stderr);

    // Should have box-drawing characters from ariadne
    assert!(text.contains("┌─") || text.contains("│"));

    // Should have error with squiggles
    assert!(text.contains("^^^^^^") || text.contains("────"));
}
```

### Manual Testing

```bash
# Test with valid document
validate-yaml --input valid.yaml --schema schema.yaml --json
# Expected: {"success": true}

# Test with invalid document (text mode)
validate-yaml --input invalid.yaml --schema schema.yaml
# Expected: Ariadne visual output with box-drawing

# Test with invalid document (JSON mode)
validate-yaml --input invalid.yaml --schema schema.yaml --json | jq .
# Expected: Structured JSON with metadata.instance_path array

# Test with nested path error
validate-yaml --input nested-error.yaml --schema schema.yaml --json | \
  jq '.errors[0].metadata.instance_path'
# Expected: [{"type": "Key", "value": "format"}, {"type": "Key", "value": "html"}]

# Test source range
validate-yaml --input type-error.yaml --schema schema.yaml --json | \
  jq '.errors[0].location'
# Expected: {"Original": {"file_id": 0, "start_offset": N, "end_offset": M}}
```

---

## Benefits of This Approach

### For Human Users 👤

1. **Beautiful visual errors** with ariadne source highlighting
2. **Consistent format** across Quarto tools (matches quarto-markdown-pandoc)
3. **Helpful hints** and structured problem statements
4. **Clear error codes** with documentation links

### For Machine Consumers 🤖

1. **Structured paths** in JSON for easy navigation
2. **Full source ranges** for accurate highlighting
3. **Stable JSON schema** for tooling integration
4. **Type-tagged metadata** for domain-specific processing

### For Maintainers 🔧

1. **Reuses existing infrastructure** (DiagnosticMessage, ariadne)
2. **Backward compatible** with current code
3. **Extensible** metadata pattern for future error types
4. **Consistent** with quarto-markdown-pandoc architecture

---

## Migration Path

### Phase 1: Internal Use (validate-yaml)

- Implement in validate-yaml binary only
- No public API changes needed
- Test JSON output with downstream consumers

### Phase 2: Library API (quarto-yaml-validation)

- Add `to_json()` / `to_text()` methods to ValidationError
- Deprecate old error_conversion pattern
- Update documentation

### Phase 3: Ecosystem Integration

- Document JSON schema for downstream tools
- Create TypeScript type definitions for JSON output
- Build LSP server integration example

---

## Open Questions

### Q1: Should we include file content in JSON output?

**Consideration**: For downstream tools, having source text would enable rich error display.

**Options**:
1. Include file content in file registry (large output)
2. Reference file by path only (tools read separately)
3. Make it optional via --include-source flag

**Recommendation**: Reference by path only. Downstream tools should read files themselves.

### Q2: Should PathSegment be in quarto-error-reporting or quarto-yaml-validation?

**Consideration**: PathSegment is specific to instance paths in YAML/JSON.

**Options**:
1. In quarto-error-reporting (generic, reusable)
2. In quarto-yaml-validation (domain-specific)

**Recommendation**: In quarto-error-reporting. Other validators (JSON schema, TOML) could reuse.

### Q3: How to handle file_id resolution in JSON?

**Current**: file_id is an opaque integer.

**Options**:
1. Include file registry in JSON output
2. Let consumers maintain their own registry
3. Add optional "files" field with id→path mapping

**Recommendation**: Option 3 - add optional files array:
```json
{
  "success": false,
  "files": [
    {"id": 0, "path": "test.yaml"}
  ],
  "errors": [...]
}
```

### Q4: Should we version the JSON schema?

**Consideration**: JSON output might evolve over time.

**Options**:
1. Add "version": "1.0" field to root
2. Use Content-Type with version
3. Don't version initially

**Recommendation**: Add version field from start:
```json
{
  "version": "1.0",
  "success": false,
  "errors": [...]
}
```

---

## Success Criteria

### Must Have ✅

- [ ] JSON output includes structured instance_path array
- [ ] JSON output includes structured schema_path array
- [ ] JSON output includes location with full range (start/end offsets)
- [ ] Text output uses ariadne for visual source highlighting
- [ ] All existing tests continue to pass
- [ ] validate-yaml binary works with both --json and text modes

### Should Have 🟡

- [ ] Documentation for JSON schema
- [ ] Example downstream consumer (Python/TypeScript)
- [ ] Integration tests for JSON output structure
- [ ] Performance: no measurable slowdown vs current implementation

### Nice to Have 🟢

- [ ] TypeScript type definitions for JSON output
- [ ] JSON schema file (.json) for validation
- [ ] LSP server integration example
- [ ] Comparison with TypeScript validator JSON output

---

## Timeline Estimate

| Phase | Description | Hours | Dependencies |
|-------|-------------|-------|--------------|
| 1 | Add metadata to DiagnosticMessage | 2-3 | None |
| 2 | Update error_conversion.rs | 1-2 | Phase 1 |
| 3 | Enable ariadne visual output | 1-2 | Phase 2 |
| 4 | Add helper methods (optional) | 1 | Phase 2 |
| **Total** | | **5-8** | |

With testing and documentation: **8-12 hours**

---

## Related Work

- **k-254**: Phase 1 (source location tracking) - ✅ Complete
- **k-257**: Phase 4 (JSON output mode) - ✅ Complete (but needs improvement)
- **k-255**: Phase 2 (ariadne visual reports) - Blocked by this work
- **k-253**: Parent issue (YAML validation error reporting)

---

## References

### Code Files

**Current Implementation**:
- `private-crates/quarto-yaml-validation/src/error.rs` - ValidationError type
- `private-crates/validate-yaml/src/error_conversion.rs` - Conversion logic
- `crates/quarto-error-reporting/src/diagnostic.rs` - DiagnosticMessage

**Reference Implementation**:
- `crates/quarto-markdown-pandoc/src/errors.rs` - Error handling patterns
- `crates/quarto-markdown-pandoc/src/main.rs` - JSON vs text output

### Design Documents

- `claude-notes/plans/2025-10-27-k-253-yaml-validation-error-reporting.md` - Original plan
- Tidyverse Error Guidelines: https://style.tidyverse.org/error-messages.html

---

## Next Steps

1. **Review this plan** with stakeholders
2. **Decide on approach** (Approach A recommended)
3. **Create sub-issues** for each phase if needed
4. **Start with Phase 1** (extend DiagnosticMessage)
5. **Iterate** based on downstream consumer feedback
