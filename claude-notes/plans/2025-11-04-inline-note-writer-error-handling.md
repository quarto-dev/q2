# Inline Note Definition Error Handling and Writer Refactoring

Date: 2025-11-04
File: claude-notes/plans/2025-11-04-inline-note-writer-error-handling.md

## Problem Summary

Inline note definitions (`[^1]: content`) are being parsed but not properly handled by the native writer, resulting in malformed output:

```qmd
With caveats[^1].

[^1]: A caveat.
```

**Current Output:**
```
[ Para [Str "With", Space, Str "caveats", Span ( "" , ["quarto-note-reference"] , [("reference-id", "1")] ) [], Str "."],  ]
```

**Problems:**
1. The `NoteReference` inline is converted to an empty `Span` (no content)
2. The `NoteDefinitionPara` block is silently dropped by the native writer
3. No error is emitted to inform the user that this construct is unsupported

## Current Architecture Analysis

### Parse Tree (from tree-sitter)
```
document
├── paragraph
│   ├── "With" (pandoc_str)
│   ├── " " (space)
│   ├── "caveats" (pandoc_str)
│   ├── "[^1]" (inline_note_reference)
│   └── "." (pandoc_str)
└── inline_ref_def
    ├── "[^1]:" (ref_id_specifier)
    └── paragraph
        ├── "A" (pandoc_str)
        ├── " " (space)
        └── "caveat." (pandoc_str)
```

### AST Processing (in treesitter.rs)

**`inline_note_reference` processing** (lines 778-856):
- Creates `Inline::NoteReference(NoteReference { id, source_info })`
- This is a Quarto extension type that should be "desugared" before writing

**`inline_ref_def` processing** (line 863):
- Calls `process_note_definition_para()`
- Creates `Block::NoteDefinitionPara(NoteDefinitionPara { id, content, source_info })`
- This is also a Quarto extension type

### Postprocessing (in postprocess.rs)

**`with_note_reference` filter** (lines 497-513):
```rust
.with_note_reference(|note_ref| {
    let mut kv = LinkedHashMap::new();
    kv.insert("reference-id".to_string(), note_ref.id.clone());
    FilterResult(
        vec![Inline::Span(Span {
            attr: (
                "".to_string(),
                vec!["quarto-note-reference".to_string()],
                kv,
            ),
            content: vec![],  // ← Empty! Should contain note content
            source_info: note_ref.source_info,
            attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        })],
        false,
    )
})
```

This converts `NoteReference` to an empty `Span` - a placeholder, not a proper desugaring.

**Missing:** No filter for `NoteDefinitionPara` blocks, so they remain in the AST.

### Native Writer (in writers/native.rs)

**Block handling** (lines 594-598):
```rust
Block::NoteDefinitionPara(_) | Block::NoteDefinitionFencedBlock(_) => {
    // Note definitions are not represented as separate blocks in Pandoc's native format.
    // The content is coalesced into Note inline elements where referenced.
    // Skip output for native writer.
}
```

Silently drops note definition blocks, assuming they've been desugared.

**Inline handling** (line 335):
```rust
_ => panic!("Unsupported inline type: {:?}", text),
```

Would panic on `NoteReference`, but it's already converted to `Span` by postprocess.

## Root Cause

1. **Incomplete Desugaring**: The postprocess filter converts `NoteReference` to an empty `Span` instead of a proper `Note` inline with content
2. **Silent Failure**: The native writer silently drops `NoteDefinitionPara` blocks
3. **No Error Reporting**: Users get malformed output with no indication that the feature is unsupported

## Proper Solution (Not Implemented Yet)

To properly support inline note definitions, we would need:
1. Collect all `NoteDefinitionPara` blocks into a map: `id -> content`
2. Replace each `NoteReference` with `Note { content }` from the map
3. Remove `NoteDefinitionPara` blocks from the document
4. Handle unresolved references (note ref without definition)

This is similar to how Pandoc processes footnotes.

## Proposed Short-Term Solution: Error Reporting

Instead of implementing full desugaring, emit clear errors when encountering unsupported constructs in writers.

### Design Goals

1. **Informative Errors**: Tell users exactly what's unsupported and where
2. **Format-Aware**: Support both console and JSON error output
3. **Non-Silent**: Never silently drop or mangle content
4. **Extensible**: Easy to add error reporting to other writers

### Architecture Changes

#### 1. Define Writer Error Types

```rust
// In crates/quarto-markdown-pandoc/src/writers/mod.rs

use crate::utils::diagnostic_collector::DiagnosticMessage;

pub enum WriterError {
    /// IO error during writing
    Io(std::io::Error),

    /// Feature not supported by this writer
    UnsupportedFeature {
        feature: String,
        writer: String,
        message: String,
        location: Option<quarto_source_map::SourceInfo>,
    },
}

impl From<std::io::Error> for WriterError {
    fn from(err: std::io::Error) -> Self {
        WriterError::Io(err)
    }
}

impl WriterError {
    pub fn to_diagnostic(&self, context: &ASTContext) -> DiagnosticMessage {
        match self {
            WriterError::Io(err) => DiagnosticMessage::error(format!("IO error: {}", err)),
            WriterError::UnsupportedFeature { feature, writer, message, location } => {
                let mut diag = DiagnosticMessage::error(format!(
                    "{} does not support {}",
                    writer, feature
                ));
                if let Some(loc) = location {
                    diag = diag.with_source_info(loc.clone(), context);
                }
                if !message.is_empty() {
                    diag = diag.with_note(message.clone());
                }
                diag
            }
        }
    }
}
```

#### 2. Update Writer Signatures

**Before:**
```rust
pub fn write<T: std::io::Write>(pandoc: &Pandoc, buf: &mut T) -> std::io::Result<()>
```

**After:**
```rust
pub fn write<T: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    buf: &mut T
) -> Result<(), WriterError>
```

This applies to:
- `crates/quarto-markdown-pandoc/src/writers/native.rs::write()`
- `crates/quarto-markdown-pandoc/src/writers/qmd.rs::write()` (if it exists)
- Any other writer functions

#### 3. Update Block/Inline Writing Functions

**Before:**
```rust
fn write_block<T: std::io::Write>(block: &Block, buf: &mut T) -> std::io::Result<()>
```

**After:**
```rust
fn write_block<T: std::io::Write>(
    block: &Block,
    context: &ASTContext,
    buf: &mut T
) -> Result<(), WriterError>
```

Similarly for `write_inline()`.

#### 4. Handle Unsupported Features

In `write_block()`:
```rust
Block::NoteDefinitionPara(note_def) => {
    return Err(WriterError::UnsupportedFeature {
        feature: "inline note definitions".to_string(),
        writer: "native".to_string(),
        message: format!(
            "Note definition [^{}] cannot be rendered in native format. \
             Use inline footnote syntax instead: ^[content]",
            note_def.id
        ),
        location: Some(note_def.source_info.clone()),
    });
}
```

In `write_inline()`:
```rust
Inline::NoteReference(note_ref) => {
    return Err(WriterError::UnsupportedFeature {
        feature: "note references".to_string(),
        writer: "native".to_string(),
        message: format!(
            "Note reference [^{}] cannot be rendered in native format. \
             This is a Quarto extension that should have been desugared.",
            note_ref.id
        ),
        location: Some(note_ref.source_info.clone()),
    });
}
```

#### 5. Update Main Binary to Handle Errors

In `crates/quarto-markdown-pandoc/src/main.rs` (or wherever the CLI is):

**Before:**
```rust
native::write(&pandoc, &mut stdout)?;
```

**After:**
```rust
match native::write(&pandoc, &context, &mut stdout) {
    Ok(()) => {
        // Success
    }
    Err(WriterError::Io(err)) => {
        // IO error - fatal
        return Err(err.into());
    }
    Err(writer_err) => {
        // Unsupported feature - format and display error
        let diagnostic = writer_err.to_diagnostic(&context);

        if json_errors {
            // Output as JSON
            eprintln!("{}", serde_json::to_string(&diagnostic)?);
        } else {
            // Output as formatted console message
            diagnostic.print(&mut std::io::stderr())?;
        }

        std::process::exit(1);
    }
}
```

#### 6. Thread Context Through Writer Calls

All internal writer functions need to receive and pass along the `context` parameter:
- `write_block()` calls itself recursively → pass context
- `write_inline()` calls itself recursively → pass context
- `write_inlines()` calls `write_inline()` → pass context
- `write_blocks()` calls `write_block()` → pass context
- `write_native_cell()` calls `write_block()` → pass context
- `write_native_table_body()` calls `write_native_rows()` → may need context
- etc.

This is a significant refactoring but necessary for proper error reporting.

## Implementation Plan

### Phase 1: Error Type Infrastructure

**Tasks:**
1. Create `WriterError` enum in `crates/quarto-markdown-pandoc/src/writers/mod.rs`
2. Implement `From<std::io::Error>` for automatic conversion
3. Implement `to_diagnostic()` method
4. Add tests for error type creation and conversion

### Phase 2: Refactor Native Writer

**Tasks:**
1. Update `write()` signature to take `context` and return `Result<(), WriterError>`
2. Update `write_block()` signature
3. Thread `context` through all block-writing functions
4. Convert all `io::Result` returns to `Result<_, WriterError>` using `?`
5. Update `write_inline()` signature
6. Thread `context` through all inline-writing functions
7. Test compilation

### Phase 3: Add Unsupported Feature Errors

**Tasks:**
1. Replace silent skip for `NoteDefinitionPara` with error return
2. Replace silent skip for `NoteDefinitionFencedBlock` with error return
3. Add error handling for `NoteReference` in `write_inline()`
4. Consider removing the postprocess filter that converts `NoteReference` to empty `Span`
   - OR keep it but add a comment explaining it's for non-native writers
5. Test with example file

### Phase 4: Update Main Binary

**Tasks:**
1. Find where `native::write()` is called
2. Add proper error handling with format-aware output
3. Thread `context` from parsing through to writing
4. Test with `--json-errors` flag
5. Test with console output (default)

### Phase 5: Testing

**Tasks:**
1. Write test with inline note definition
2. Verify it produces clear error message (not panic, not silent)
3. Test console error format
4. Test JSON error format
5. Add to error corpus if appropriate
6. Document the limitation in user-facing docs

### Phase 6: Other Writers (If Applicable)

**Tasks:**
1. Check if QMD writer exists and needs same treatment
2. Check if JSON writer exists and needs same treatment
3. Apply same refactoring pattern to other writers

## Alternative Approaches Considered

### 1. Implement Full Note Desugaring

**Pros:**
- Would properly support inline note definitions
- Matches Pandoc behavior

**Cons:**
- More complex implementation
- Need to handle edge cases (missing definitions, duplicate IDs, etc.)
- Not requested by user for this iteration

**Decision:** Defer to future work

### 2. Panic on Unsupported Features

**Pros:**
- Simple to implement
- Forces attention to the problem

**Cons:**
- Bad user experience
- No source location information
- Not recoverable

**Decision:** Rejected

### 3. Silent Skip (Current Behavior)

**Pros:**
- Simple
- Doesn't break the pipeline

**Cons:**
- Produces malformed output
- No user feedback
- Debugging nightmare

**Decision:** This is what we're fixing

### 4. Warnings Instead of Errors

**Pros:**
- Doesn't stop the pipeline
- Provides feedback

**Cons:**
- Still produces malformed output
- Users might ignore warnings

**Decision:** Use errors, not warnings (user wants errors)

## Testing Strategy

### Test Cases

1. **Basic inline note definition**
   ```qmd
   With caveats[^1].

   [^1]: A caveat.
   ```
   Expected: Error with source location pointing to `[^1]: A caveat.`

2. **Multiple note definitions**
   ```qmd
   Text[^1] more[^2].

   [^1]: First.
   [^2]: Second.
   ```
   Expected: Error on first note definition encountered

3. **Note definition with complex content**
   ```qmd
   Text[^note].

   [^note]: This is a longer note
     with multiple lines.
   ```
   Expected: Error with helpful message

4. **JSON error output**
   ```bash
   cargo run -p quarto-markdown-pandoc -- -i test.qmd --json-errors
   ```
   Expected: Valid JSON error object

5. **Console error output**
   ```bash
   cargo run -p quarto-markdown-pandoc -- -i test.qmd
   ```
   Expected: Formatted error with source location and helpful message

### Integration Tests

- Add test file to `crates/quarto-markdown-pandoc/tests/should_fail/`
- Verify error is produced (not panic, not success with bad output)
- Verify error message contains expected information

## Technical Considerations

### 1. Error vs Result Propagation

Using `Result<(), WriterError>` allows:
- Propagating IO errors with `?` operator
- Early return on unsupported features
- Clean error handling in caller

### 2. Context Threading

The `ASTContext` contains:
- Current file ID for source locations
- File path for error messages
- Any other parsing context needed

Threading it through all writer functions adds a parameter but is necessary for meaningful error messages.

### 3. Backward Compatibility

This is a breaking API change for the writer modules. If other code depends on the old signatures, it will need updating.

Check:
- Internal uses in same crate
- Tests
- Any external crates (unlikely for internal writer module)

### 4. Performance

Adding error handling adds minimal overhead:
- One extra parameter (pointer/reference)
- Result enum returns (zero-cost abstraction)
- No performance impact in success case

### 5. Future Extensibility

This pattern can be extended to:
- Other unsupported features
- Warnings (using `Vec<Diagnostic>` return type)
- Multiple errors (collect and return all)
- Writer-specific validations

## Documentation Updates Needed

1. **Code comments**: Update function docs for new signatures
2. **User docs**: Document that inline note definitions are not supported
3. **Migration guide**: If changing public API, document migration
4. **Error corpus**: Add example errors for common mistakes

## Open Questions

1. **Should we remove the postprocess filter that converts `NoteReference` to `Span`?**
   - Keeping it allows other writers to handle note refs differently
   - Removing it makes the error detection cleaner
   - **Decision needed from user**

2. **Should warnings vs errors be configurable?**
   - Some users might want to allow unsupported features with warnings
   - **Decision:** Start with errors, add flags later if needed

3. **Should we accumulate multiple errors or fail on first?**
   - Fail-fast is simpler
   - Accumulating is more helpful for users
   - **Decision:** Start with fail-fast, enhance later if needed

4. **Where exactly is the `context` available in main.rs?**
   - Need to trace through the code to find where parsing creates context
   - **Action:** Investigate during implementation

## Success Criteria

- [ ] Running example file produces clear error (not panic, not malformed output)
- [ ] Error message includes source location
- [ ] Error message suggests alternative (inline footnote syntax)
- [ ] JSON error output is well-formed and parseable
- [ ] Console error output is readable and helpful
- [ ] All existing tests still pass
- [ ] New test added for inline note definition error
- [ ] Code compiles with no warnings
- [ ] Documentation updated

## Timeline Estimate

- Phase 1: 1-2 hours (error type infrastructure)
- Phase 2: 3-4 hours (refactor native writer signatures)
- Phase 3: 1-2 hours (add unsupported feature errors)
- Phase 4: 1-2 hours (update main binary)
- Phase 5: 2-3 hours (testing and validation)
- Phase 6: 1-2 hours (other writers if needed)

**Total:** 9-15 hours of work

## Next Steps

1. Review plan with user
2. Get decision on open questions
3. Begin Phase 1 implementation
4. Test incrementally after each phase
