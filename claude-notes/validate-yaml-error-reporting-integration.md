# Integration of quarto-error-reporting into validate-yaml Binary

<!-- quarto-error-code-audit-ignore-file -->

**Date**: 2025-10-13
**Context**: Design for incorporating structured error reporting into the validate-yaml CLI tool

## Current State

### validate-yaml Binary
- Uses `ValidationError` from `quarto-yaml-validation`
- Simple text output to stderr
- Reports: message, instance path, schema path, location

### quarto-error-reporting Crate
- **Phase 1 Complete**: Core types (DiagnosticMessage, builder API, error codes)
- **Phase 2 Planned**: Rendering integration (ariadne, JSON)
- Provides structured, tidyverse-style error messages
- Error code system (Q-<subsystem>-<number>)

## Design Goals

1. **Enhance User Experience**: Replace plain text errors with structured, helpful diagnostic messages
2. **Enable Error Codes**: Add Q-1-xxx error codes for YAML validation errors
3. **Prepare for Future**: Design with ariadne integration in mind (Phase 2)
4. **Maintain Simplicity**: Keep the binary simple while leveraging rich error infrastructure

## Proposed Approach

### Option 1: Convert ValidationError → DiagnosticMessage (Recommended)

Create a conversion function that transforms `ValidationError` into `DiagnosticMessage`:

```rust
impl ValidationError {
    pub fn to_diagnostic(&self) -> DiagnosticMessage {
        DiagnosticMessageBuilder::error(&self.message)
            .with_code(self.infer_error_code())  // Based on error type
            .problem(&self.message)
            .add_detail(format!("At path: {}", self.instance_path))
            .add_detail(format!("Schema constraint: {}", self.schema_path))
            .build()
    }
}
```

**Pros**:
- Clean separation: validation logic stays in quarto-yaml-validation
- Error reporting is a separate concern handled by quarto-error-reporting
- Easy to extend with error codes and hints
- Prepares for Phase 2 ariadne rendering

**Cons**:
- Extra conversion step
- Error codes must be inferred from error message/type

### Option 2: ValidationError Implements DiagnosticMessage Trait

Create a trait that ValidationError implements:

```rust
pub trait AsDiagnostic {
    fn as_diagnostic(&self) -> DiagnosticMessage;
}
```

**Pros**:
- Standard pattern for error conversion
- Extensible to other error types

**Cons**:
- Adds dependency from quarto-yaml-validation to quarto-error-reporting
- May not be needed if only validate-yaml uses it

### Option 3: Direct DiagnosticMessage in Validator (Not Recommended)

Change `ValidationError` to contain `DiagnosticMessage`:

```rust
pub struct ValidationError {
    pub diagnostic: DiagnosticMessage,
    pub instance_path: InstancePath,
    // ...
}
```

**Cons**:
- Tight coupling between validation and error reporting
- Over-engineering for library code
- Validation should be independent of presentation

## Recommended Solution: Option 1

**Implementation in validate-yaml/src/main.rs**:

```rust
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};

fn validation_error_to_diagnostic(error: &ValidationError) -> DiagnosticMessage {
    let mut builder = DiagnosticMessageBuilder::error("YAML Validation Failed")
        .with_code(infer_error_code(error))
        .problem(&error.message);

    // Add instance path as detail
    if !error.instance_path.is_empty() {
        builder = builder.add_detail(format!(
            "At document path: `{}`{{.path}}",
            error.instance_path
        ));
    }

    // Add schema path as info
    if !error.schema_path.is_empty() {
        builder = builder.add_info(format!(
            "Schema constraint at: {}",
            error.schema_path
        ));
    }

    // Add location as detail
    if let Some(loc) = &error.location {
        builder = builder.add_detail(format!(
            "In file `{}`{{.file}} at line {}, column {}",
            loc.file, loc.line, loc.column
        ));
    }

    // Add hints based on error type
    if let Some(hint) = suggest_fix(error) {
        builder = builder.add_hint(hint);
    }

    builder.build()
}

fn infer_error_code(error: &ValidationError) -> &'static str {
    // Map common error patterns to Q-1-xxx codes
    if error.message.contains("Missing required") {
        "Q-1-10"  // Missing required property
    } else if error.message.contains("Expected") && error.message.contains("got") {
        "Q-1-11"  // Type mismatch
    } else if error.message.contains("must be one of") {
        "Q-1-12"  // Invalid enum value
    } else {
        "Q-1-99"  // Generic validation error
    }
}

fn suggest_fix(error: &ValidationError) -> Option<String> {
    // Provide contextual hints based on error patterns
    if error.message.contains("Missing required property") {
        Some("Add the required property to your YAML document?".to_string())
    } else if error.message.contains("Expected boolean") {
        Some("Use `true` or `false` (YAML 1.2 standard)?".to_string())
    } else {
        None
    }
}
```

**Error Output** (for now, simple text; later ariadne):

```
Error: YAML Validation Failed (Q-1-10)

Problem: Missing required property 'author'

  ✖ At document path: `(root)`
  ℹ Schema constraint at: object
  ✖ In file `document.yaml` at line 2, column 6

  ? Add the required property to your YAML document?

See https://quarto.org/docs/errors/Q-1-10 for more information
```

## Error Code Allocation

**Subsystem 1: YAML and Configuration** (Q-1-xxx)

Validation errors:
- `Q-1-10`: Missing required property
- `Q-1-11`: Type mismatch (expected X, got Y)
- `Q-1-12`: Invalid enum value
- `Q-1-13`: Array length constraint violation
- `Q-1-14`: String pattern mismatch
- `Q-1-15`: Number range violation
- `Q-1-16`: Object property count violation
- `Q-1-17`: Unresolved schema reference
- `Q-1-99`: Generic validation error

Schema errors:
- `Q-1-1`: YAML syntax error (already defined)
- `Q-1-2`: Invalid schema type
- `Q-1-3`: Invalid schema structure
- `Q-1-4`: Missing required schema field

## Implementation Plan

### Step 1: Add Error Code Mapping
1. Create `error_codes.rs` in validate-yaml with `infer_error_code()` and `suggest_fix()`
2. Update error_catalog.json in quarto-error-reporting with Q-1-10 through Q-1-17

### Step 2: Create Conversion Function
1. Add `validation_error_to_diagnostic()` in validate-yaml/src/main.rs
2. Convert ValidationError to DiagnosticMessage before displaying

### Step 3: Simple Text Rendering (Phase 1)
1. Implement basic text rendering of DiagnosticMessage
2. Show: title, code, problem, details (with bullets), hints, docs URL

### Step 4: Prepare for Phase 2
1. Structure code so ariadne rendering can be swapped in later
2. Add `--format` flag: `text`, `json` (for future `ariadne`)

## Example Usage After Integration

**Valid document**:
```bash
$ validate-yaml --schema schema.yaml --input valid.yaml
✓ Validation successful
  Input: valid.yaml
  Schema: schema.yaml
```

**Invalid document** (simple text for now):
```bash
$ validate-yaml --schema schema.yaml --input invalid.yaml
Error: YAML Validation Failed (Q-1-10)

Problem: Missing required property 'author'

  ✖ At document path: `(root)`
  ℹ Schema constraint at: object
  ✖ In file `invalid.yaml` at line 2, column 6

  ? Add the required property to your YAML document?

See https://quarto.org/docs/errors/Q-1-10 for more information

Exit code: 1
```

**With JSON output** (future):
```bash
$ validate-yaml --schema schema.yaml --input invalid.yaml --format json
{
  "code": "Q-1-10",
  "title": "YAML Validation Failed",
  "kind": "Error",
  "problem": "Missing required property 'author'",
  "details": [
    {"kind": "Error", "content": "At document path: `(root)`"},
    {"kind": "Info", "content": "Schema constraint at: object"},
    {"kind": "Error", "content": "In file `invalid.yaml` at line 2, column 6"}
  ],
  "hints": ["Add the required property to your YAML document?"],
  "docs_url": "https://quarto.org/docs/errors/Q-1-10"
}
```

## Benefits

1. **Better UX**: Structured, tidyverse-style error messages
2. **Searchable**: Error codes enable Googling "Quarto Q-1-10"
3. **Actionable**: Hints provide guidance on fixing errors
4. **Documented**: Each error code links to detailed docs
5. **Future-Ready**: Prepared for ariadne visual error reports (Phase 2)
6. **Consistent**: Uses same error infrastructure as rest of Quarto

## Dependencies

### validate-yaml/Cargo.toml
```toml
[dependencies]
quarto-yaml = { path = "../quarto-yaml" }
quarto-yaml-validation = { path = "../quarto-yaml-validation" }
quarto-error-reporting = { path = "../quarto-error-reporting" }  # NEW
anyhow.workspace = true
clap = { version = "4.5", features = ["derive"] }
```

## Notes

- **No changes to quarto-yaml-validation**: ValidationError stays as-is
- **Conversion happens in binary**: validate-yaml does the mapping
- **Error codes are inferred**: Based on error message patterns (for now)
- **Future enhancement**: Validator could include error codes directly, but not required

## Questions for Discussion

1. Should we add error code hints to the validator itself, or always infer in the binary?
2. What's the priority for Phase 2 ariadne integration?
3. Should we support multiple output formats (`--format text|json`) from the start?
