# JSON Writer Diagnostic Support - k-378

<!-- quarto-error-code-audit-ignore-file -->

**Date**: 2025-11-21
**Issue**: k-378 - Add diagnostic error reporting to JSON writer
**Blocked**: k-375 (fixing JSON writer panics)

## Problem

The JSON writer currently has no diagnostic error reporting infrastructure. When defensive error cases are encountered (e.g., editorial marks that weren't desugared, CaptionBlocks that weren't processed), it uses:
- `panic!()` - crashes the program (k-375 original issue)
- `eprintln!()` - prints to stderr, invisible to `--json-errors`

Neither approach works with Quarto's error reporting infrastructure.

## Current State Analysis

### API Signatures

**Native Writer** (has diagnostics):
```rust
pub fn write<T: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    buf: &mut T,
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>>

fn write_impl(..., errors: &mut Vec<DiagnosticMessage>) -> std::io::Result<()>

fn write_block(..., errors: &mut Vec<DiagnosticMessage>) -> std::io::Result<()>

fn write_inline(..., errors: &mut Vec<DiagnosticMessage>) -> std::io::Result<()>
```

**JSON Writer** (no diagnostics):
```rust
pub fn write<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
) -> std::io::Result<()>  // ❌ No diagnostic support

fn write_pandoc(pandoc: &Pandoc, context: &ASTContext, config: &JsonConfig) -> Value

fn write_block(block: &Block, serializer: &mut SourceInfoSerializer) -> Value

fn write_inline(inline: &Inline, serializer: &mut SourceInfoSerializer) -> Value
```

### Key Architectural Difference

**Native Writer**:
- Writes directly to buffer using `write!()` macros
- Errors are accumulated during traversal
- Returns `Result<(), Vec<DiagnosticMessage>>` at the end

**JSON Writer**:
- Builds JSON `Value` tree first (pure data)
- Serializes `Value` to JSON at the very end
- No error accumulation mechanism during tree building

## Proposed Solution

### Phase 1: Add Error Accumulation Infrastructure

Add errors vector to `SourceInfoSerializer` since it's already threaded everywhere:

```rust
struct SourceInfoSerializer<'a> {
    pool: Vec<SerializableSourceInfo>,
    id_map: HashMap<*const SourceInfo, usize>,
    context: &'a ASTContext,
    config: &'a JsonConfig,
    errors: Vec<quarto_error_reporting::DiagnosticMessage>,  // ✅ NEW
}
```

**Benefits**:
- Already passed to all write functions
- Natural place to accumulate errors during traversal
- No need to change all function signatures

### Phase 2: Update Public API

```rust
pub fn write<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>> {  // ✅ Changed
    write_with_config(pandoc, context, writer, &JsonConfig::default())
}

pub fn write_with_config<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
    config: &JsonConfig,
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>> {  // ✅ Changed
    let mut serializer = SourceInfoSerializer::new(context, config);

    // Build JSON (accumulates errors in serializer)
    let json = write_pandoc_impl(pandoc, &mut serializer);

    // Try to write JSON
    if let Err(e) = serde_json::to_writer(writer, &json) {
        return Err(vec![
            DiagnosticMessageBuilder::error("IO error during JSON write")
                .with_code("Q-3-1")
                .problem(format!("Failed to write JSON: {}", e))
                .build()
        ]);
    }

    // Return accumulated errors if any
    if !serializer.errors.is_empty() {
        return Err(serializer.errors);
    }

    Ok(())
}
```

### Phase 3: Replace eprintln! with errors.push()

**Current code** (line 538):
```rust
eprintln!("Warning: Shortcode '{}' reached JSON writer unexpectedly...", shortcode.name);
```

**New code**:
```rust
serializer.errors.push(
    DiagnosticMessageBuilder::error("Shortcode not supported in JSON format")
        .with_code("Q-3-30")  // Reuse native writer error codes
        .problem(format!("Cannot render shortcode `{{{{< {} >}}}}` in JSON", shortcode.name))
        .add_detail("Shortcodes are Quarto-specific and not representable in Pandoc JSON")
        .add_hint("Use native format or process shortcodes before writing JSON")
        .build()
);
```

### Phase 4: Error Code Strategy

**Option A: Reuse Native Writer Codes** (Recommended)
- Q-3-20: BlockMetadata not supported
- Q-3-21: CaptionBlock not supported
- Q-3-30: Shortcode not supported
- Q-3-31: Unprocessed NoteReference
- Q-3-32: Standalone Attr not supported
- Q-3-33: Unprocessed Insert
- Q-3-34: Unprocessed Delete
- Q-3-35: Unprocessed Highlight
- Q-3-36: Unprocessed EditComment

**Rationale**: Same semantic errors across writers. Error catalog entries say "not supported in this output format", which is true for both native and JSON.

**Option B: Create Separate JSON-Specific Codes**
- Q-3-40 through Q-3-48

**Rationale**: Allows different messaging per writer if needed.

**Recommendation**: Option A - simpler, consistent UX.

## Implementation Plan

### Step 1: Update SourceInfoSerializer (30 min)

```rust
impl<'a> SourceInfoSerializer<'a> {
    fn new(context: &'a ASTContext, config: &'a JsonConfig) -> Self {
        SourceInfoSerializer {
            pool: Vec::new(),
            id_map: HashMap::new(),
            context,
            config,
            errors: Vec::new(),  // ✅ Add
        }
    }
}
```

### Step 2: Update write() signatures (15 min)

Change public API return types as shown in Phase 2.

### Step 3: Replace eprintln! calls (1 hour)

Locations to fix:
1. Line 538: Shortcode → Q-3-30
2. Line 547: NoteReference → Q-3-31
3. Line 556: Attr → Q-3-32
4. Line 562: Insert → Q-3-33
5. Line 569: Delete → Q-3-34
6. Line 576: Highlight → Q-3-35
7. Line 583: EditComment → Q-3-36
8. Line 1030: CaptionBlock → Q-3-21
9. Line 1121: Non-MetaMap → New error Q-3-40?

### Step 4: Update Tests (1 hour)

Existing tests need to handle new return type:

**Before**:
```rust
#[test]
fn test_write_json() {
    let mut buf = Vec::new();
    write(&pandoc, &context, &mut buf).unwrap();
    // ...
}
```

**After**:
```rust
#[test]
fn test_write_json() {
    let mut buf = Vec::new();
    write(&pandoc, &context, &mut buf).unwrap();  // Still works - Ok(()) case
    // ...
}

#[test]
fn test_write_json_with_shortcode_error() {
    let pandoc_with_shortcode = /* ... */;
    let mut buf = Vec::new();

    let result = write(&pandoc_with_shortcode, &context, &mut buf);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, Some("Q-3-30".to_string()));
}
```

### Step 5: Integration Testing (30 min)

Test with actual qmd files containing problematic constructs:

```bash
# Should emit Q-3-30 error
echo '{{< shortcode >}}' | quarto-markdown-pandoc -t json --json-errors

# Should emit Q-3-33 error (if Insert not desugared)
echo '{++insert++}' | quarto-markdown-pandoc -t json --json-errors
```

## Error Messages

### Q-3-30: Shortcode

```
Error: Shortcode not supported in JSON format

Cannot render shortcode `{{< myshortcode >}}` in JSON format

Shortcodes are Quarto-specific syntax not represented in Pandoc's JSON format.

Hint: Use native format or ensure shortcodes are processed before JSON output
```

### Q-3-33: Unprocessed Insert (Defensive)

```
Error: Unprocessed Insert markup

Insert markup `{++...++}` was not desugared during postprocessing

Editorial marks should be converted to Span nodes during postprocessing.
This may indicate a bug or a filter that bypassed postprocessing.

Hint: Ensure postprocessing is enabled or use a Lua filter to handle editorial marks
```

### Q-3-21: CaptionBlock (Defensive)

```
Error: Caption block not supported

Standalone caption block cannot be rendered in JSON format

Caption blocks should be attached to figures or tables during postprocessing.
This may indicate a postprocessing issue or filter-generated orphaned caption.

Hint: Check for bugs in postprocessing or filters producing orphaned captions
```

## Backward Compatibility

### Breaking Change

The `write()` function signature changes from:
```rust
fn write(...) -> std::io::Result<()>
```

to:
```rust
fn write(...) -> Result<(), Vec<DiagnosticMessage>>
```

### Impact Analysis

**Internal callers** (in quarto-markdown-pandoc):
- Need to update error handling
- May need to propagate/convert errors

**External callers** (quarto-cli, etc.):
- Already handling Result type
- Need to handle DiagnosticMessage vec instead of io::Error

**Migration path**:
```rust
// Old code
match json::write(&pandoc, &context, &mut buf) {
    Ok(()) => { /* success */ }
    Err(io_err) => { /* handle io error */ }
}

// New code
match json::write(&pandoc, &context, &mut buf) {
    Ok(()) => { /* success */ }
    Err(diagnostics) => {
        // Can print with --json-errors or human-readable
        for diag in diagnostics {
            eprintln!("{}", diag);
        }
    }
}
```

## Testing Strategy

### Unit Tests

1. **Successful write** - No errors accumulated
2. **Editorial mark errors** - Each type (Insert, Delete, Highlight, EditComment)
3. **Shortcode error** - Defensive
4. **NoteReference error** - Defensive
5. **CaptionBlock error** - Defensive
6. **Multiple errors** - Accumulation works
7. **IO error** - JSON serialization fails

### Integration Tests

1. Real qmd with shortcodes → Q-3-30 error
2. Real qmd with editorial marks (bypassing desugar) → Q-3-33 etc.
3. `--json-errors` flag produces proper JSON error output

## Risks and Mitigations

### Risk 1: Breaking API Change

**Mitigation**: Document clearly, provide migration guide, version bump

### Risk 2: Performance Impact

Adding error accumulation might slow down JSON generation.

**Mitigation**: Errors vec is only populated in edge cases (defensive errors). Normal path unchanged.

### Risk 3: Testing Coverage

Hard to trigger some defensive cases (like CaptionBlock reaching writer).

**Mitigation**: Add explicit test helpers that bypass postprocessing to create problematic ASTs.

## Estimated Effort

- **Planning**: 1 hour (done)
- **Implementation**: 2-3 hours
- **Testing**: 1 hour
- **Documentation**: 30 minutes
- **Total**: 4-5 hours

## Success Criteria

1. ✅ All `panic!()` removed from JSON writer
2. ✅ All `eprintln!()` replaced with DiagnosticMessage
3. ✅ `--json-errors` flag works with JSON writer errors
4. ✅ Error codes Q-3-20, Q-3-21, Q-3-30-36 reused from native writer
5. ✅ All existing tests pass with updated API
6. ✅ New tests cover all error cases
7. ✅ Integration tests verify end-to-end error flow

## Follow-up Work

After k-378 is complete, revisit k-375 to:
1. Remove temporary eprintln! calls
2. Verify all panics are gone
3. Close k-375 as complete
