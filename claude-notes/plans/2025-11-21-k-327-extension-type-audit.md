# Extension Type Writer Audit - k-327

**Date**: 2025-11-21
**Issue**: k-327 - Audit other Quarto extension types for silent writer failures

## Executive Summary

Audit found **critical crash bugs** in the native writer. Instead of emitting helpful errors for unsupported Quarto extension types, the writer uses `panic!()` which crashes the entire program.

## Quarto Extension Types Identified

### Block Extensions
1. **BlockMetadata** - YAML metadata blocks
2. **NoteDefinitionPara** - ✅ Fixed in k-326
3. **NoteDefinitionFencedBlock** - ✅ Fixed in k-326
4. **CaptionBlock** - Figure/table caption blocks

### Inline Extensions
1. **Shortcode** - `{{< shortcode >}}` syntax
2. **NoteReference** - `[^1]` references (should be converted in postprocess)
3. **Attr** - Standalone attributes (for headings/tables)
4. **Insert** - CriticMarkup `{++text++}`
5. **Delete** - CriticMarkup `{--text--}`
6. **Highlight** - CriticMarkup `{==text==}`
7. **EditComment** - CriticMarkup `{>>text<<}`

## Native Writer Audit Results

**File**: `crates/quarto-markdown-pandoc/src/writers/native.rs`

### Critical Issue 1: Block Panic

**Location**: Line 667

```rust
fn write_block<T: std::io::Write>(
    block: &Block,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
    match block {
        // ... handled types ...
        Block::NoteDefinitionPara(note_def) => { /* proper error */ }
        Block::NoteDefinitionFencedBlock(note_def) => { /* proper error */ }
        _ => panic!("Unsupported block type in native writer: {:?}", block),
        //   ^^^^^ CRASHES on BlockMetadata and CaptionBlock!
    }
    Ok(())
}
```

**Problem**: Any unhandled block type **crashes** the program instead of emitting a DiagnosticMessage.

**Affected Extension Types**:
- ✗ **BlockMetadata** - CRASH
- ✗ **CaptionBlock** - CRASH

### Critical Issue 2: Inline Panic

**Location**: Line 355

```rust
fn write_inline<T: std::io::Write>(
    text: &Inline,
    context: &crate::pandoc::ast_context::ASTContext,
    buf: &mut T,
    errors: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
) -> std::io::Result<()> {
    match text {
        // ... handled types ...
        _ => panic!("Unsupported inline type: {:?}", text),
        //   ^^^^^ CRASHES on all extension inline types!
    }
    Ok(())
}
```

**Problem**: Any unhandled inline type **crashes** the program instead of emitting a DiagnosticMessage.

**Affected Extension Types**:
- ✗ **Shortcode** - CRASH
- ✗ **NoteReference** - CRASH (defensive - should be converted in postprocess)
- ✗ **Attr** - CRASH
- ✗ **Insert** - CRASH
- ✗ **Delete** - CRASH
- ✗ **Highlight** - CRASH
- ✗ **EditComment** - CRASH

### Additional Panic Found

**Location**: Line 94

```rust
fn write_native_colwidth<T: std::io::Write>(
    colwidth: &crate::pandoc::ColWidth,
    buf: &mut T,
) -> std::io::Result<()> {
    match colwidth {
        crate::pandoc::ColWidth::Default => write!(buf, "ColWidthDefault"),
        crate::pandoc::ColWidth::Percentage(percentage) => {
            // FIXME
            panic!("ColWidthPercentage is not implemented yet: {}", percentage);
        }
    }
}
```

**Problem**: Tables with percentage column widths crash.

## Severity Assessment

### Critical (P0 - Crashes)
1. **BlockMetadata in native writer** - Crashes program
2. **CaptionBlock in native writer** - Crashes program
3. **Shortcode in native writer** - Crashes program
4. **Insert in native writer** - Crashes program
5. **Delete in native writer** - Crashes program
6. **Highlight in native writer** - Crashes program
7. **EditComment in native writer** - Crashes program
8. **Attr inline in native writer** - Crashes program
9. **ColWidthPercentage in native writer** - Crashes program

### High (P1 - Defensive)
10. **NoteReference in native writer** - Should never reach writer (postprocess converts it), but crashes if it does

## Expected Behavior

Each unsupported extension type should:

1. **Not crash** - Replace `panic!()` with error accumulation
2. **Emit clear error** - Use `DiagnosticMessageBuilder`
3. **Provide source location** - Use the type's `source_info` field
4. **Suggest alternatives** - Give users actionable hints
5. **Skip gracefully** - Continue processing other elements

### Example (from k-326 fix):

```rust
Block::NoteDefinitionPara(note_def) => {
    errors.push(
        DiagnosticMessageBuilder::error("Inline note definitions not supported")
            .with_code("Q-3-10")
            .problem(format!(
                "Cannot render inline note definition `[^{}]` in native format",
                note_def.id
            ))
            .with_location(note_def.source_info.clone())
            .add_detail("...")
            .add_hint("Use inline footnote syntax instead: `^[your note content here]`")
            .build()
    );
    // Skip this block - don't write anything
}
```

## Recommended Fixes

### Fix 1: Replace Block Panic with Exhaustive Match

**Before**:
```rust
_ => panic!("Unsupported block type in native writer: {:?}", block),
```

**After**:
```rust
Block::BlockMetadata(meta) => {
    errors.push(
        DiagnosticMessageBuilder::error("Block metadata not supported in native format")
            .with_code("Q-3-20")
            .problem("Cannot render YAML metadata block in native format")
            .with_location(meta.source_info.clone())
            .add_hint("Metadata blocks are only supported in JSON output")
            .build()
    );
}
Block::CaptionBlock(caption) => {
    errors.push(
        DiagnosticMessageBuilder::error("Caption block not supported in native format")
            .with_code("Q-3-21")
            .problem("Cannot render standalone caption block in native format")
            .with_location(caption.source_info.clone())
            .add_detail("Caption blocks should be attached to figures or tables")
            .build()
    );
}
```

### Fix 2: Replace Inline Panic with Exhaustive Match

Similar pattern for all inline extensions (Q-3-30 through Q-3-36).

### Fix 3: Replace ColWidth Panic

```rust
crate::pandoc::ColWidth::Percentage(percentage) => {
    // Don't panic - this is a valid Pandoc construct
    write!(buf, "ColWidth {}", percentage)?;
}
```

## Error Code Allocation

**Reserve Q-3-20 through Q-3-40** for extension type errors:

- Q-3-20: BlockMetadata not supported
- Q-3-21: CaptionBlock not supported
- Q-3-30: Shortcode not supported
- Q-3-31: NoteReference (defensive check)
- Q-3-32: Attr inline not supported
- Q-3-33: Insert not supported
- Q-3-34: Delete not supported
- Q-3-35: Highlight not supported
- Q-3-36: EditComment not supported

## Implementation Strategy

### Option A: Single Comprehensive Issue (Recommended)

Create **one issue** to fix all panic statements in native writer:
- "Replace panic!() with proper error handling in native writer"
- Fixes all blocks + all inlines + colwidth
- Estimated: 4-6 hours
- Single PR, easier to review

### Option B: Separate Issues Per Type

Create individual issues as originally planned:
- k-XXX: BlockMetadata error handling
- k-XXX: CaptionBlock error handling
- k-XXX: Shortcode error handling
- ... (9 total issues)

**Tradeoff**: More granular tracking but more overhead

## Next Steps

1. ✅ Complete audit of native writer
2. ⏳ Audit other writers (qmd, html, json)
3. ⏳ Decide on Option A vs Option B
4. ⏳ Create beads issues
5. ⏳ Implement fixes
6. ⏳ Add tests for each extension type
7. ⏳ Update error catalog

## Questions

1. **Should we fix all panic!() in one issue or separate them?**
   - Leaning toward Option A (single issue) for efficiency

2. **What about other writers (qmd, html, json)?**
   - Need to audit those next
   - May have different issues (silent drops vs panics)

3. **Should ColWidthPercentage be supported?**
   - Yes - it's valid Pandoc, not a Quarto extension
   - Should write as `ColWidth <percentage>` not panic

4. **What error message for editorial marks?**
   - Suggest converting to tracked changes in Word/Google Docs?
   - Suggest using Lua filter for custom handling?
