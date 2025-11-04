# Inline Note Definition Error Handling Using DiagnosticMessage

Date: 2025-11-04
File: claude-notes/plans/2025-11-04-inline-note-writer-error-handling-v2.md

**Beads Issue:** k-326
**Related Issues:** k-327 (audit other extension types)

**Previous Version:** 2025-11-04-inline-note-writer-error-handling.md (superseded)

## Problem Summary

Inline note definitions (`[^1]: content`) are being parsed but not properly handled by the native writer, resulting in malformed output with no user feedback. We need to emit clear errors when writers encounter unsupported constructs.

**Example:**
```qmd
With caveats[^1].

[^1]: A caveat.
```

**Current Output (Silent Failure):**
```
[ Para [Str "With", Space, Str "caveats", Span ( "" , ["quarto-note-reference"] , [("reference-id", "1")] ) [], Str "."],  ]
```

**Problems:**
1. Note reference becomes an empty Span (no content)
2. Note definition is silently dropped
3. No error message to user

## Architecture: Use Existing DiagnosticMessage Infrastructure

The codebase already has a comprehensive error reporting system in `quarto-error-reporting`:

### Current Pattern (from main.rs)

**Readers** return: `Result<(Pandoc, ASTContext, Vec<DiagnosticMessage>), Vec<DiagnosticMessage>>`
- Ok: parsed document + warnings
- Err: parse errors

**Main** formats diagnostics:
```rust
if args.json_errors {
    for diagnostic in diagnostics {
        println!("{}", diagnostic.to_json());
    }
} else {
    for diagnostic in diagnostics {
        eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
    }
}
```

**Writers** currently return: `io::Result<()>`
- Only propagate IO errors
- No diagnostic support

### New Pattern for Writers

**Design Decision: Error Accumulation (Option A)**

Writers will use **error accumulation** to collect multiple feature errors before failing:
- Fatal IO errors return immediately
- Feature errors (unsupported constructs) are accumulated in a mutable vector
- Thread `errors: &mut Vec<DiagnosticMessage>` parameter through all functions
- Return `io::Result<()>` - IO errors propagate, feature errors accumulate

**Rationale:** This allows users to see all unsupported constructs in one pass, while still failing fast on fatal errors.

## Proposed Design

### 1. Update Writer Signatures

**Before:**
```rust
pub fn write<T: std::io::Write>(pandoc: &Pandoc, buf: &mut T) -> std::io::Result<()>
```

**After:**
```rust
pub fn write<T: std::io::Write>(
    pandoc: &Pandoc,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T
) -> Result<(), Vec<quarto_error_reporting::DiagnosticMessage>>
```

**Changes:**
- Add `context` parameter for source location access
- Return `Result<(), Vec<DiagnosticMessage>>` for accumulated errors
- Internal functions use `errors: &mut Vec<DiagnosticMessage>` parameter
- IO errors return immediately as `io::Result<()>`

**Apply to:**
- `crates/quarto-markdown-pandoc/src/writers/native.rs`
- `crates/quarto-markdown-pandoc/src/writers/qmd.rs`
- `crates/quarto-markdown-pandoc/src/writers/html.rs`
- Any other writer modules

### 2. Update Internal Writer Functions

All internal functions that write blocks/inlines need updated signatures:

**Before:**
```rust
fn write_block<T: std::io::Write>(block: &Block, buf: &mut T) -> std::io::Result<()>
fn write_inline<T: std::io::Write>(inline: &Inline, buf: &mut T) -> std::io::Result<()>
```

**After:**
```rust
fn write_block<T: std::io::Write>(
    block: &Block,
    context: &ASTContext,
    buf: &mut T,
    errors: &mut Vec<DiagnosticMessage>
) -> io::Result<()>

fn write_inline<T: std::io::Write>(
    inline: &Inline,
    context: &ASTContext,
    buf: &mut T,
    errors: &mut Vec<DiagnosticMessage>
) -> io::Result<()>
```

**Pattern:**
- IO errors propagate via `?` operator (fatal, stop immediately)
- Feature errors push to `errors` vector and continue
- Functions skip unsupported blocks/inlines after logging error

### 3. Handle IO Errors

IO errors propagate directly using `?` operator:

```rust
// IO errors are fatal - propagate immediately
write!(buf, "Para [")?;
write_inlines(&para.content, context, buf, errors)?;
write!(buf, "]")?;
```

No wrapping needed - IO errors short-circuit and main.rs will convert to DiagnosticMessage.

### 4. Detect Unsupported Features

**In `write_block()` for NoteDefinitionPara:**

```rust
Block::NoteDefinitionPara(note_def) => {
    // Feature error - accumulate and continue
    errors.push(
        DiagnosticMessageBuilder::error("Inline note definitions not supported")
            .with_code("Q-3-10")
            .problem(format!(
                "Cannot render inline note definition `[^{}]` in native format",
                note_def.id
            ))
            .with_location(note_def.source_info.clone())
            .add_detail(
                "Inline note definitions require the note content to be coalesced \
                 into the reference location, which is not yet implemented"
            )
            .add_hint("Use inline footnote syntax instead: `^[your note content here]`")
            .add_hint(format!(
                "Or use a Lua filter to process `[^{}]` references",
                note_def.id
            ))
            .build()
    );
    // Skip this block - don't write anything
    Ok(())
}
```

**In `write_block()` for NoteDefinitionFencedBlock:**

```rust
Block::NoteDefinitionFencedBlock(note_def) => {
    // Feature error - accumulate and continue
    errors.push(
        DiagnosticMessageBuilder::error("Fenced note definitions not supported")
            .with_code("Q-3-11")
            .problem(format!(
                "Cannot render fenced note definition `[^{}]` in native format",
                note_def.id
            ))
            .with_location(note_def.source_info.clone())
            .add_detail(
                "Fenced note definitions require the note content to be coalesced \
                 into the reference location, which is not yet implemented"
            )
            .add_hint("Use inline footnote syntax instead: `^[your note content here]`")
            .build()
    );
    // Skip this block
    Ok(())
}
```

**Note:** `NoteReference` should never reach the writer - the postprocess filter converts it to a proper `Span` with class `"quarto-note-reference"` and attributes. If it does reach the writer, it's a bug. However, we can add defensive error handling:

**In `write_inline()` for NoteReference (defensive):**

```rust
Inline::NoteReference(note_ref) => {
    // This should never happen - postprocess converts NoteReference to Span
    errors.push(
        DiagnosticMessageBuilder::error("Unresolved note reference")
            .with_code("Q-3-12")
            .problem(format!(
                "Note reference `[^{}]` was not converted during postprocessing",
                note_ref.id
            ))
            .with_location(note_ref.source_info.clone())
            .add_detail(
                "This is a bug in the postprocessor. Note references should be \
                 converted to Span before reaching the writer."
            )
            .add_hint("Please report this as a bug with a minimal reproducible example")
            .build()
    );
    // Skip this inline
    Ok(())
}
```

### 5. Update Main Binary

**In main.rs, update writer calls:**

**Before:**
```rust
match args.to.as_str() {
    "json" => writers::json::write(&pandoc, &context, &mut buf),
    "native" => writers::native::write(&pandoc, &mut buf),
    "markdown" | "qmd" => writers::qmd::write(&pandoc, &mut buf),
    "html" => writers::html::write(&pandoc, &mut buf),
    _ => {
        eprintln!("Unknown output format: {}", args.to);
        return;
    }
}
.unwrap();
```

**After:**
```rust
let writer_result = match args.to.as_str() {
    "json" => writers::json::write(&pandoc, &context, &mut buf),
    "native" => writers::native::write(&pandoc, &context, &mut buf),
    "markdown" | "qmd" => writers::qmd::write(&pandoc, &context, &mut buf),
    "html" => writers::html::write(&pandoc, &context, &mut buf),
    _ => {
        eprintln!("Unknown output format: {}", args.to);
        std::process::exit(1);
    }
};

if let Err(err) = writer_result {
    // err can be either:
    // - Vec<DiagnosticMessage> (feature errors)
    // - io::Error (wrapped in Result conversion)

    let diagnostics = match err {
        // If it's already diagnostics, use them
        diagnostics @ _ if /* check if Vec<DiagnosticMessage> */ => diagnostics,
        // If it's an IO error, wrap it
        io_err => vec![
            DiagnosticMessageBuilder::error("IO error during write")
                .with_code("Q-3-1")
                .problem(format!("Failed to write output: {}", io_err))
                .build()
        ]
    };

    // Format and output errors
    if args.json_errors {
        for diagnostic in diagnostics {
            eprintln!("{}", diagnostic.to_json());
        }
    } else {
        for diagnostic in diagnostics {
            eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
        }
    }
    std::process::exit(1);
}
```

**Note:** The writer returns `Result<(), Vec<DiagnosticMessage>>`. The inner implementation uses `io::Result<()>` which gets caught and converted to DiagnosticMessage at the writer boundary.

### 6. Error Code Allocation

Reserve error code range for writer subsystem:

- **Q-3-x**: Writer errors (subsystem 3)
  - Q-3-1: IO error during write
  - Q-3-10: Inline note definition not supported
  - Q-3-11: Fenced note definition not supported
  - Q-3-12: Unresolved note reference (defensive check)
  - Q-3-20+: Other unsupported features (reserve range)

**Note:** Q-2 is already used for markdown parser errors. Writers use Q-3.

Update `crates/quarto-error-reporting/error_catalog.json` to document these codes.

## Implementation Plan

### Phase 1: Infrastructure Setup (1-2 hours)

**Tasks:**
1. ✅ Review existing DiagnosticMessage API
2. ✅ Review main.rs error handling pattern
3. Document error codes Q-2-1 through Q-2-12 in catalog.rs
4. Create helper function/macro for IO error wrapping
5. Write tests for helper function

**Deliverable:** Error code documentation and IO error helpers

### Phase 2: Refactor Native Writer Signatures (2-3 hours)

**Tasks:**
1. Update `write()` signature in native.rs
2. Update `write_block()` signature
3. Update `write_inline()` signature
4. Update all other internal functions to receive `context`
5. Thread `context` parameter through all function calls
6. Replace `io::Result` with `Result<_, Vec<DiagnosticMessage>>`
7. Replace all `write!()` calls with error-wrapping version
8. Compile and fix errors

**Deliverable:** Native writer compiles with new signatures

### Phase 3: Add Unsupported Feature Diagnostics (1-2 hours)

**Tasks:**
1. Replace silent skip for `NoteDefinitionPara` with diagnostic error
2. Replace silent skip for `NoteDefinitionFencedBlock` with diagnostic error
3. Add diagnostic for `NoteReference` in inline writer
4. Test with example file: verify error messages
5. Verify source locations are accurate
6. Verify error suggestions are helpful

**Deliverable:** Writers emit clear errors for unsupported features

### Phase 4: Update Main Binary (1 hour)

**Tasks:**
1. Update writer calls in main.rs to pass `context`
2. Add error handling for writer diagnostics
3. Format errors using `to_json()` or `to_text()` pattern
4. Test with `--json-errors` flag
5. Test with console output (default)
6. Ensure exit code is non-zero on error

**Deliverable:** Main binary properly handles and formats writer errors

### Phase 5: Testing (2-3 hours)

**Test Cases:**

1. **Basic inline note definition**
   ```qmd
   With caveats[^1].

   [^1]: A caveat.
   ```
   Expected: Error with source location, helpful message, suggestions

2. **Fenced note definition**
   ```qmd
   Text[^note].

   [^note]:
   : This is a longer note
   : with multiple paragraphs.
   ```
   Expected: Error on fenced definition

3. **Multiple note definitions**
   ```qmd
   Text[^1] more[^2].

   [^1]: First.
   [^2]: Second.
   ```
   Expected: Error on first note encountered

4. **Console output test**
   ```bash
   cargo run -p quarto-markdown-pandoc -- -i test-inline-note.qmd -t native
   ```
   Expected: Beautiful Ariadne-formatted error with source highlighting

5. **JSON output test**
   ```bash
   cargo run -p quarto-markdown-pandoc -- -i test-inline-note.qmd -t native --json-errors
   ```
   Expected: Valid JSON diagnostic object

6. **Error code documentation**
   - Verify Q-3-10 is documented in catalog
   - Verify docs URL works (if implemented)

7. **Regression tests**
   - Ensure all existing tests still pass
   - No breaking changes to successful conversions

**Deliverable:** All tests passing, error messages verified

### Phase 6: Other Writers (1-2 hours)

Apply same pattern to other writers if they exist and need it:

**Tasks:**
1. Check QMD writer (writers/qmd.rs)
2. Check HTML writer (writers/html.rs)
3. Apply same refactoring if needed
4. Test each writer

**Deliverable:** Consistent error handling across all writers

### Phase 7: Documentation (1 hour)

**Tasks:**
1. Update writer function documentation
2. Add code comments explaining diagnostic creation
3. Document error codes in catalog
4. Update user-facing docs about inline note limitations
5. Add CHANGELOG entry

**Deliverable:** Comprehensive documentation

## Expected Output Examples

### Console Output (Default)

```
Error [Q-3-10]: Inline note definitions not supported
   ╭─[test-inline-note.qmd:3:1]
   │
 3 │ [^1]: A caveat.
   │ ───────┬───────
   │        ╰── Cannot render inline note definition `[^1]` in native format
───╯
ℹ Inline note definitions require the note content to be coalesced into the reference location, which is not yet implemented
? Use inline footnote syntax instead: `^[your note content here]`
? Or use a Lua filter to process `[^1]` references
```

### JSON Output (--json-errors)

```json
{
  "kind": "error",
  "code": "Q-3-10",
  "title": "Inline note definitions not supported",
  "problem": {
    "type": "markdown",
    "content": "Cannot render inline note definition `[^1]` in native format"
  },
  "details": [
    {
      "kind": "info",
      "content": {
        "type": "markdown",
        "content": "Inline note definitions require the note content to be coalesced into the reference location, which is not yet implemented"
      }
    }
  ],
  "hints": [
    {
      "type": "markdown",
      "content": "Use inline footnote syntax instead: `^[your note content here]`"
    },
    {
      "type": "markdown",
      "content": "Or use a Lua filter to process `[^1]` references"
    }
  ],
  "location": {
    "Original": {
      "file_id": 0,
      "start_offset": 20,
      "end_offset": 35
    }
  }
}
```

## Benefits of This Design

1. **Consistent with existing patterns**: Uses same infrastructure as reader errors
2. **No new abstractions**: Leverages `quarto-error-reporting` crate
3. **Format-agnostic**: `DiagnosticMessage` handles both console and JSON
4. **Rich error messages**: Ariadne integration for beautiful terminal output
5. **Source locations**: Full mapping support through SourceContext
6. **Tidyverse-style**: Follows best practices for error message structure
7. **Extensible**: Easy to add more unsupported feature checks

## Technical Considerations

### 1. IO Error Handling

Every `write!()` call needs error wrapping. Options:

**Option A: Helper function**
```rust
write!(buf, "text").map_err(io_error)?;
```

**Option B: Helper macro**
```rust
write_or_err!(buf, "text");
```

**Recommendation**: Use helper function initially, add macro if it becomes too verbose.

### 2. Multiple Errors vs Fail-Fast

Current design fails on first error. Future enhancement could collect multiple errors:

```rust
let mut errors = Vec::new();
for block in pandoc.blocks {
    if let Err(mut e) = write_block(block, context, buf) {
        errors.append(&mut e);
    }
}
if !errors.is_empty() {
    return Err(errors);
}
```

**Recommendation**: Start with fail-fast, add accumulation if users request it.

### 3. Context Availability

The `context` contains `source_context` which is needed for:
- Mapping source locations to file paths
- Ariadne rendering of source snippets

Ensure `context` flows from main through to writer.

### 4. Backward Compatibility

This changes the writer API. Impact:

- **Internal to crate**: No external API break
- **Tests**: Need to update test calls to pass context
- **Binary**: main.rs needs updates (already part of plan)

### 5. Performance

Minimal impact:
- Extra parameter (pointer) per function call
- Result wrapping (zero-cost abstraction)
- Only creates diagnostics on error path

## Resolved Questions

### 1. Should we remove the postprocess filter for NoteReference? ✅ RESOLVED

**Decision:** Keep the filter - it's correct behavior!

The postprocess filter that converts `NoteReference` to `Span` is a **proper desugaring step**. It produces a Span with:
- Class: `"quarto-note-reference"`
- Attribute: `("reference-id", id)`
- Content: Empty (intentionally - the reference is marked in attributes)

This is not the source of the problem. The native writer can handle Spans just fine. The issue is with `NoteDefinitionPara` and `NoteDefinitionFencedBlock` blocks, which **don't** have a desugaring step.

### 2. Should other Quarto extension types also error? ✅ RESOLVED

**Decision:** Yes, but in separate issues.

Created issue **k-327** to audit other Quarto extension types after this work is complete. Each type found should get its own issue for tracking.

### 3. Error accumulation vs fail-fast? ✅ RESOLVED

**Decision:** Use error accumulation (Option A).

Thread `errors: &mut Vec<DiagnosticMessage>` through writer functions:
- IO errors (fatal): Return immediately via `?` operator
- Feature errors: Push to `errors` vec and continue processing
- Users see all unsupported constructs in one pass

### 4. Should JSON writer also get this treatment?

**Action:** Review JSON writer during implementation. It may already handle all node types correctly since it outputs Pandoc JSON directly. Check during Phase 6.

## Success Criteria

- [ ] Running example file produces clear error (not panic, not silent, not malformed)
- [ ] Error message includes source location with Ariadne rendering
- [ ] Error suggests alternative (inline footnote syntax)
- [ ] Error includes helpful details about why it's unsupported
- [ ] JSON error output is well-formed and parseable
- [ ] Console error output is beautiful and readable
- [ ] All existing tests still pass
- [ ] New tests added for inline note definition errors
- [ ] Error codes documented in catalog
- [ ] Code compiles with no warnings
- [ ] Documentation updated

## Timeline Estimate

- Phase 1: 1-2 hours (infrastructure setup)
- Phase 2: 2-3 hours (refactor native writer signatures)
- Phase 3: 1-2 hours (add unsupported feature diagnostics)
- Phase 4: 1 hour (update main binary)
- Phase 5: 2-3 hours (testing and validation)
- Phase 6: 1-2 hours (other writers if needed)
- Phase 7: 1 hour (documentation)

**Total:** 9-14 hours of work

## Next Steps

1. Review plan with user
2. Get decision on open questions
3. Begin Phase 1 implementation
4. Test incrementally after each phase
5. Submit for review before moving to next writer
