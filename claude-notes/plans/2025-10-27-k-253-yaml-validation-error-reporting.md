# k-253: YAML Validation Error Reporting Improvements

**Date**: 2025-10-27
**Status**: Planning
**Priority**: 1

## Executive Summary

Improve error messages from `quarto-yaml-validation` to provide:
1. **Rich contextual hints** with proper source location tracking
2. **Visual source highlighting** using ariadne
3. **Structured JSON output** for programmatic consumption
4. **Better error messages** following tidyverse style guide

Use `validate-yaml` binary as an isolated test bed for these improvements.

---

## Current State Analysis

### Architecture Overview

```
validate-yaml (binary)
    ├─→ quarto-yaml-validation (validation engine)
    │   ├─→ quarto-yaml (parser with source tracking)
    │   └─→ error.rs (ValidationError type)
    ├─→ quarto-error-reporting (error infrastructure)
    │   ├─→ DiagnosticMessage (structured errors)
    │   ├─→ Builder API (tidyverse-style)
    │   ├─→ ariadne integration (visual source context)
    │   └─→ JSON serialization
    └─→ error_conversion.rs (ValidationError → DiagnosticMessage)
```

### Current Validation Flow

1. **Parse YAML** → `YamlWithSourceInfo` (quarto-yaml)
2. **Validate** → `ValidationError` (quarto-yaml-validation)
3. **Convert** → `DiagnosticMessage` (validate-yaml/error_conversion.rs)
4. **Display** → Text output (validate-yaml/main.rs)

### What Works Well ✅

1. **Basic validation**: Type checking, required properties, constraints all work
2. **Error codes**: Q-1-xxx codes are inferred correctly
3. **Hint generation**: Basic hints are provided based on error type
4. **Structure**: Clean separation between validation and error reporting
5. **Builder API**: `DiagnosticMessageBuilder` provides excellent ergonomics
6. **Test data**: Simple schemas and documents exist for testing

### Current Issues ❌

#### 1. Source Location Tracking is Broken

**Current output:**
```
  ✖ In file `<unknown>` at line 0, column 5
```

**Root cause:** `ValidationError::with_yaml_node()` doesn't properly extract location:
```rust
// From private-crates/quarto-yaml-validation/src/error.rs:136
self.location = Some(SourceLocation {
    file: "<unknown>".to_string(),  // ❌ File tracking not implemented
    line: 0,                         // ❌ Would need SourceContext to compute
    column: node.source_info.start_offset(), // ❌ Using offset as proxy
});
```

**Issue:** `YamlWithSourceInfo` has rich `SourceInfo` but it's not being used properly:
- `SourceInfo` tracks transformations through the parsing pipeline
- `SourceContext` is needed to map offsets to line/column
- File path information is in `SourceContext`, not passed to validator

#### 2. No Visual Source Context

**Current:** Plain text errors without source highlighting

**Expected:** ariadne-style visual reports like:
```
Error [Q-1-11]: Type mismatch
  ┌─ test.yaml:3:7
  │
3 │ year: "not a number"
  │       ^^^^^^^^^^^^^^ Expected number, got string
  │
  = Use a numeric value without quotes?
```

**Blocker:** Requires proper `SourceContext` integration (see Issue #1)

#### 3. Limited Contextual Information

**Current error:**
```
Problem: Missing required property 'author'
  ✖ At document root
```

**Could be:**
```
Problem: Missing required property 'author'
  ✖ At document root
  ℹ Required by schema at line 18 in simple-schema.yaml
  ℹ The 'author' field must be a string (see schema line 7)
```

#### 4. No JSON Output Mode

**Gap:** validate-yaml only outputs text, but we need JSON for:
- Programmatic error handling
- Editor integrations (LSP)
- CI/CD pipelines
- Structured logging

#### 5. AnyOf Error Pruning Not Implemented

**TypeScript has sophisticated pruning** (validator.ts:174-290):
- Heuristics to select "best" error from multiple anyOf branches
- Prefers errors about missing required fields
- Sorts by error quality and span size

**Rust validator** (validator.rs:396-420):
- Collects all errors from failed branches
- No pruning or quality selection
- TODO comment acknowledges this gap

---

## Comparison with TypeScript Validator

### TypeScript Strengths

1. **AnnotatedParse with source tracking**
   ```typescript
   interface AnnotatedParse {
     result: JSONValue;      // The parsed value
     source: MappedString;   // Source tracking
     start: number;          // Offset in source
     end: number;            // End offset
     components: AnnotatedParse[]; // Children
   }
   ```

2. **Sophisticated anyOf error selection**
   - Prefers "required field" errors over "invalid property" errors
   - Uses error quality scoring based on schema tags
   - Minimizes total span of errors

3. **Rich error context**
   ```typescript
   function createLocalizedError({
     violatingObject,
     source,
     message,
     instancePath,
     schemaPath
   }): LocalizedError
   ```
   Creates `TidyverseError` with:
   - Full source context with line/column
   - Proper file name tracking
   - Location ranges

### Rust Strengths

1. **Better type safety** - Rust's type system prevents many edge cases
2. **Modern error infrastructure** - `quarto-error-reporting` is more advanced
3. **ariadne integration** - Visual error reports (when source context works)
4. **Builder API** - More ergonomic than TypeScript version
5. **JSON serialization** - Built into `DiagnosticMessage`

### Key Learning: We Need SourceContext Threading

The main difference is that TypeScript passes `source: MappedString` everywhere.
Rust equivalent would be passing `&SourceContext` to the validator.

---

## Proposed Improvements

### Phase 1: Fix Source Location Tracking (Essential) 🔴

**Goal:** Get proper file names and line numbers in error messages

**Changes:**

1. **Thread SourceContext through validation**
   ```rust
   // Current
   pub fn validate(
       value: &YamlWithSourceInfo,
       schema: &Schema,
       registry: &SchemaRegistry,
   ) -> ValidationResult<()>

   // Proposed
   pub fn validate(
       value: &YamlWithSourceInfo,
       schema: &Schema,
       registry: &SchemaRegistry,
       source_ctx: &SourceContext,  // NEW
   ) -> ValidationResult<()>
   ```

2. **Store SourceContext in ValidationContext**
   ```rust
   pub struct ValidationContext<'a> {
       registry: &'a SchemaRegistry,
       source_ctx: &'a SourceContext,  // NEW
       instance_path: InstancePath,
       schema_path: SchemaPath,
       errors: Vec<ValidationError>,
   }
   ```

3. **Properly extract location in with_yaml_node**
   ```rust
   pub fn with_yaml_node(mut self, node: YamlWithSourceInfo, ctx: &SourceContext) -> Self {
       // Map offset to line/column using SourceContext
       if let Some(mapped) = node.source_info.map_offset(0, ctx) {
           if let Some(file) = ctx.get_file(mapped.file_id) {
               self.location = Some(SourceLocation {
                   file: file.path.clone(),
                   line: mapped.location.row + 1,      // 1-indexed for display
                   column: mapped.location.column + 1, // 1-indexed for display
               });
           }
       }
       self.yaml_node = Some(node);
       self
   }
   ```

4. **Update validate-yaml to provide SourceContext**
   ```rust
   // Build SourceContext from parsed YAML
   let mut source_ctx = SourceContext::new();
   let file_id = source_ctx.add_file(
       input_filename.to_string(),
       Some(input_content.clone())
   );

   // Validate with context
   match validate(&input_yaml, &schema, &registry, &source_ctx) {
       // ...
   }
   ```

**Test:** After these changes, error output should show:
```
  ✖ In file `invalid-document.yaml` at line 3, column 7
```

**Estimate:** 3-4 hours (requires careful API threading)

---

### Phase 2: Enable ariadne Visual Reports (High Value) 🟡

**Goal:** Show beautiful source-highlighted errors with ariadne

**Changes:**

1. **Store SourceInfo in ValidationError**
   ```rust
   pub struct ValidationError {
       pub message: String,
       pub instance_path: InstancePath,
       pub schema_path: SchemaPath,
       pub source_info: Option<SourceInfo>,  // NEW (instead of yaml_node)
       pub location: Option<SourceLocation>,
   }
   ```

2. **Update error_conversion to set location on DiagnosticMessage**
   ```rust
   pub fn validation_error_to_diagnostic(
       error: &ValidationError
   ) -> DiagnosticMessage {
       let mut builder = DiagnosticMessageBuilder::error("YAML Validation Failed")
           // ...

       // Add source location for ariadne rendering
       if let Some(source_info) = &error.source_info {
           builder = builder.with_location(source_info.clone());
       }

       builder.build()
   }
   ```

3. **Update display_diagnostic to pass SourceContext**
   ```rust
   fn display_diagnostic(diagnostic: &DiagnosticMessage, ctx: &SourceContext) {
       // to_text() will use ariadne if location + context provided
       let text = diagnostic.to_text(Some(ctx));
       eprintln!("{}", text);
   }
   ```

**Expected output:**
```
Error [Q-1-11]: Type mismatch
  ┌─ invalid-document.yaml:3:7
  │
3 │ year: "not a number"
  │       ^^^^^^^^^^^^^^ Expected number, got string
  │
  = Use a numeric value without quotes?
```

**Test cases:**
- Single error with source highlight
- Multiple errors in same file
- Nested object errors
- Array item errors

**Estimate:** 2-3 hours (mostly integration work)

---

### Phase 3: Enhanced Error Messages (Nice to Have) 🟢

**Goal:** Richer contextual information in error messages

**Improvements:**

1. **Schema location hints**
   ```rust
   builder = builder.add_info(format!(
       "Required by schema property `{}` (line {})",
       prop_name,
       schema_line  // Would need schema source tracking
   ));
   ```

2. **Expected value hints**
   ```rust
   // For enums
   builder = builder.add_info(format!(
       "Allowed values: {}",
       schema.enum_values.join(", ")
   ));

   // For numbers
   builder = builder.add_info(format!(
       "Must be between {} and {}",
       schema.minimum, schema.maximum
   ));
   ```

3. **Property name suggestions** (for unknown properties)
   ```rust
   use strsim::levenshtein;

   if let Some(similar) = find_similar_property(unknown_key, schema.properties) {
       builder = builder.add_hint(format!(
           "Did you mean `{}`?",
           similar
       ));
   }
   ```

4. **Schema path breadcrumbs**
   ```rust
   // Instead of: "Schema constraint: properties > format > properties > html"
   // Show: "In format.html configuration"
   builder = builder.add_detail(
       format!("In {}", format_schema_path(&error.schema_path))
   );
   ```

**Estimate:** 4-5 hours (incremental improvements)

---

### Phase 4: JSON Output Mode (Required for Integration) 🔴

**Goal:** Structured machine-readable error output

**Changes:**

1. **Add --json flag to validate-yaml**
   ```rust
   #[derive(Parser, Debug)]
   struct Args {
       // ...

       /// Output errors as JSON instead of text
       #[arg(long)]
       json: bool,
   }
   ```

2. **JSON error output**
   ```rust
   Err(error) => {
       let diagnostic = validation_error_to_diagnostic(&error);

       if args.json {
           // Structured JSON output
           let json = serde_json::json!({
               "success": false,
               "errors": [diagnostic.to_json()]
           });
           println!("{}", serde_json::to_string_pretty(&json)?);
       } else {
           // Human-readable text output
           display_diagnostic(&diagnostic, &source_ctx);
       }

       process::exit(1);
   }
   ```

3. **Success output**
   ```rust
   Ok(()) => {
       if args.json {
           println!(r#"{"success": true}"#);
       } else {
           println!("✓ Validation successful");
           println!("  Input: {}", args.input.display());
           println!("  Schema: {}", args.schema.display());
       }
       Ok(())
   }
   ```

**Example JSON output:**
```json
{
  "success": false,
  "errors": [
    {
      "kind": "error",
      "title": "YAML Validation Failed",
      "code": "Q-1-10",
      "problem": {
        "type": "markdown",
        "content": "Missing required property 'author'"
      },
      "details": [
        {
          "kind": "error",
          "content": {
            "type": "markdown",
            "content": "At document path: `(root)`"
          }
        },
        {
          "kind": "info",
          "content": {
            "type": "markdown",
            "content": "Schema constraint: object"
          }
        }
      ],
      "hints": [
        {
          "type": "markdown",
          "content": "Add the `author` property to your YAML document?"
        }
      ],
      "location": {
        "Original": {
          "file_id": 0,
          "start_offset": 0,
          "end_offset": 65
        }
      }
    }
  ]
}
```

**Test cases:**
- Valid document (success: true)
- Single error
- Multiple errors
- Errors with locations
- Errors without locations

**Estimate:** 2-3 hours

---

### Phase 5: AnyOf Error Pruning (Advanced) 🟡

**Goal:** Better error messages for anyOf validation failures

**Strategy:** Port TypeScript heuristics to Rust

**Implementation:**

1. **Error quality scoring**
   ```rust
   fn error_quality(error: &ValidationError) -> i32 {
       // Lower is better

       // Check schema tags for explicit error-importance
       if let Some(importance) = error.get_error_importance() {
           return importance;
       }

       // Heuristics based on error type
       if error.schema_path.ends_with("propertyNames") {
           return 10; // Invalid property names are less helpful
       }

       if error.schema_path.ends_with("required") {
           return 0; // Missing required fields are most helpful
       }

       if error.schema_path.ends_with("type") {
           if error.message.contains("null") {
               return 10; // "Try null" is usually unhelpful
           }
           return 1; // Type errors are helpful
       }

       1 // Default
   }
   ```

2. **Prefer required field errors over property name errors**
   ```rust
   fn prune_anyof_errors(error_groups: Vec<Vec<ValidationError>>)
       -> Vec<ValidationError>
   {
       // If one group has "required" errors and another has "propertyNames" errors,
       // prefer the "required" group

       let has_required = |group: &[ValidationError]| {
           group.iter().any(|e| e.schema_path.ends_with("required"))
       };

       let has_property_names = |group: &[ValidationError]| {
           group.iter().any(|e| e.schema_path.contains("propertyNames"))
       };

       if error_groups.iter().any(|g| has_required(g)) &&
          error_groups.iter().any(|g| has_property_names(g)) {
           // Return only the "required" errors
           return error_groups.into_iter()
               .filter(|g| has_required(g))
               .flatten()
               .collect();
       }

       // Otherwise, select best error group by quality
       select_best_error_group(error_groups)
   }
   ```

3. **Update validate_any_of in validator.rs**
   ```rust
   fn validate_any_of(
       value: &YamlWithSourceInfo,
       schema: &crate::schema::AnyOfSchema,
       context: &mut ValidationContext,
   ) -> ValidationResult<()> {
       let original_error_count = context.errors.len();
       let mut error_groups = Vec::new();

       for subschema in &schema.schemas {
           let mut sub_context = ValidationContext::new(
               context.registry,
               context.source_ctx
           );
           sub_context.instance_path = context.instance_path.clone();
           sub_context.schema_path = context.schema_path.clone();

           if validate_generic(value, subschema, &mut sub_context).is_ok() {
               // Success! Clear any errors from failed attempts
               context.errors.truncate(original_error_count);
               return Ok(());
           }

           // Store this group of errors
           error_groups.push(sub_context.errors);
       }

       // All subschemas failed - select best errors
       let best_errors = prune_anyof_errors(error_groups);
       context.errors.extend(best_errors);

       Err(context.errors[original_error_count].clone())
   }
   ```

**Test cases:**
- anyOf with required field vs. invalid property
- anyOf with multiple type options
- anyOf with complex nested schemas
- Compare output to TypeScript validator

**Estimate:** 6-8 hours (complex heuristics, needs careful testing)

---

## Test Plan

### Test Suite Structure

```
private-crates/validate-yaml/tests/
  ├── integration_tests.rs        # End-to-end validation tests
  ├── error_reporting_tests.rs    # Error message quality tests
  ├── source_location_tests.rs    # Source tracking tests
  └── json_output_tests.rs        # JSON format tests

private-crates/validate-yaml/test-data/
  ├── schemas/
  │   ├── simple.yaml             # Basic types
  │   ├── nested.yaml             # Nested objects
  │   ├── anyof.yaml              # anyOf constructs
  │   └── complex.yaml            # Real-world complexity
  └── documents/
      ├── valid/                  # Should pass validation
      ├── type-errors/            # Type mismatches
      ├── required-errors/        # Missing required fields
      ├── nested-errors/          # Errors in nested objects
      └── anyof-errors/           # anyOf validation failures
```

### Test Categories

#### 1. Source Location Tests
```rust
#[test]
fn test_error_shows_correct_file_name() {
    // Error should show "test.yaml", not "<unknown>"
}

#[test]
fn test_error_shows_correct_line_number() {
    // Should show line 3, not line 0
}

#[test]
fn test_nested_object_error_location() {
    // Error in format.html.toc should show correct line
}

#[test]
fn test_array_item_error_location() {
    // Error in authors[2].name should show correct line
}
```

#### 2. Error Message Quality Tests
```rust
#[test]
fn test_missing_required_field_message() {
    let output = run_validator("missing-author.yaml", "schema.yaml");
    assert!(output.contains("Missing required property 'author'"));
    assert!(output.contains("Add the `author` property"));
}

#[test]
fn test_type_mismatch_message() {
    let output = run_validator("wrong-type.yaml", "schema.yaml");
    assert!(output.contains("Expected number, got string"));
    assert!(output.contains("Use a numeric value without quotes"));
}
```

#### 3. Visual Output Tests
```rust
#[test]
fn test_ariadne_source_highlighting() {
    let output = run_validator_text("invalid.yaml", "schema.yaml");
    assert!(output.contains("┌─"));           // Box drawing
    assert!(output.contains("invalid.yaml")); // File name
    assert!(output.contains("│"));            // Line prefix
}
```

#### 4. JSON Output Tests
```rust
#[test]
fn test_json_output_valid_document() {
    let output = run_validator_json("valid.yaml", "schema.yaml");
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(json["success"], true);
}

#[test]
fn test_json_output_error_structure() {
    let output = run_validator_json("invalid.yaml", "schema.yaml");
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();

    assert_eq!(json["success"], false);
    assert!(json["errors"].is_array());

    let error = &json["errors"][0];
    assert_eq!(error["kind"], "error");
    assert!(error["code"].is_string());
    assert!(error["title"].is_string());
    assert!(error["problem"].is_object());
    assert!(error["details"].is_array());
}
```

#### 5. AnyOf Pruning Tests
```rust
#[test]
fn test_anyof_prefers_required_field_errors() {
    // Schema with anyOf: [type A requires 'foo', type B allows 'bar']
    // Document with 'bar' but missing 'foo'
    // Should report "missing required field 'foo'", not "invalid property 'bar'"
}

#[test]
fn test_anyof_type_selection() {
    // anyOf: [string, number, boolean]
    // Document has object
    // Should report the most reasonable type error
}
```

### Regression Tests

Compare output to TypeScript validator on same inputs:
```bash
# TypeScript
deno run quarto-cli/src/core/lib/yaml-validation/validate.ts \
  --schema test-schema.yaml \
  --input test-doc.yaml

# Rust
cargo run --bin validate-yaml \
  --schema test-schema.yaml \
  --input test-doc.yaml
```

---

## Implementation Strategy

### Recommended Order

1. **Phase 1** (Essential): Fix source location tracking
   - Blocking issue for everything else
   - High impact, moderate complexity
   - **Do this first**

2. **Phase 4** (Essential): JSON output mode
   - Required for integration testing
   - Independent of other phases
   - **Do this second**

3. **Phase 2** (High value): ariadne visual reports
   - Depends on Phase 1
   - High user value
   - **Do this third**

4. **Phase 3** (Nice to have): Enhanced messages
   - Can be done incrementally
   - Doesn't block other work
   - **Do this fourth, or in parallel with testing**

5. **Phase 5** (Advanced): anyOf pruning
   - Complex, needs careful testing
   - Lower priority than visual improvements
   - **Do this last, or defer**

### Time Estimates

| Phase | Description | Hours | Priority |
|-------|-------------|-------|----------|
| 1 | Source location tracking | 3-4 | 🔴 Essential |
| 2 | ariadne visual reports | 2-3 | 🟡 High Value |
| 3 | Enhanced error messages | 4-5 | 🟢 Nice to Have |
| 4 | JSON output mode | 2-3 | 🔴 Essential |
| 5 | anyOf error pruning | 6-8 | 🟡 Advanced |
| | **Total** | **17-23** | |

With testing and documentation: **25-30 hours**

---

## Success Criteria

### Minimum Viable (Phases 1 + 4)

- [ ] Error messages show actual file names (not `<unknown>`)
- [ ] Error messages show correct line/column numbers (not `line 0`)
- [ ] JSON output mode works (`--json` flag)
- [ ] JSON output has stable schema
- [ ] All existing tests continue to pass

### Target (Phases 1 + 2 + 4)

- [ ] All minimum viable criteria
- [ ] ariadne visual source highlighting works
- [ ] Error messages show beautiful box-drawing output
- [ ] Multi-location errors highlight all relevant locations
- [ ] Comprehensive test suite for error reporting

### Stretch (All phases)

- [ ] All target criteria
- [ ] Enhanced contextual hints (schema location, suggestions)
- [ ] Property name spell-checking
- [ ] anyOf error pruning with quality heuristics
- [ ] Error messages comparable in quality to TypeScript validator

---

## Open Questions

1. **Should we store `SourceInfo` or `YamlWithSourceInfo` in `ValidationError`?**
   - `SourceInfo`: Lighter weight, sufficient for location
   - `YamlWithSourceInfo`: Provides access to actual YAML value
   - **Recommendation**: `SourceInfo` is sufficient

2. **Do we need to track schema source locations?**
   - Would enable "required by schema line 18" messages
   - Would require parsing schema with source tracking
   - **Recommendation**: Defer to Phase 3 if time permits

3. **Should JSON output be pretty-printed by default?**
   - Pretty: Human readable, larger
   - Compact: Machine readable, smaller
   - **Recommendation**: Add `--json-compact` flag, default to pretty

4. **How should we handle multiple errors?**
   - Show all errors (overwhelming?)
   - Show first error only (might miss context)
   - **Recommendation**: Show all by default, add `--first-error` flag

5. **Should we implement LSP server next?**
   - Natural next step after JSON output
   - Enables real-time validation in editors
   - **Recommendation**: Separate issue, defer for now

---

## Related Issues

- **k-31**: File tracking in SourceContext (prerequisite for Phase 1)
- **k-34**: TypeScript/WASM integration (would benefit from JSON output)
- **k-1**: Error reporting migration (related infrastructure)
- **bd-7**: Pandoc AST to ANSI writer (related to error display)

---

## References

### Code Files

**Core Validation:**
- `private-crates/quarto-yaml-validation/src/validator.rs` - Validation engine
- `private-crates/quarto-yaml-validation/src/error.rs` - Error types

**Error Conversion:**
- `private-crates/validate-yaml/src/error_conversion.rs` - ValidationError → DiagnosticMessage
- `private-crates/validate-yaml/src/error_codes.rs` - Error code inference

**Error Reporting:**
- `crates/quarto-error-reporting/src/diagnostic.rs` - Core types
- `crates/quarto-error-reporting/src/builder.rs` - Builder API

**TypeScript Reference:**
- `external-sources/quarto-cli/src/core/lib/yaml-validation/validator.ts` - TypeScript validator
- `external-sources/quarto-cli/src/core/lib/yaml-validation/errors.ts` - Error creation

### Documentation

- Tidyverse Error Style Guide: https://style.tidyverse.org/error-messages.html
- ariadne crate: https://docs.rs/ariadne/
- quarto-source-map design: `claude-notes/source-context-typescript-integration.md`

---

## Next Steps

1. **Review this plan** with user
2. **Prioritize phases** based on user needs
3. **Create sub-issues** for each phase
4. **Start with Phase 1** (source location tracking)
5. **Iterate** on error message quality based on real-world usage
