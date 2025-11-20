# Deep Analysis: Downsides of Option 1 (Adjust Diagnostic Locations)

## Critical Downside #1: AST Nodes Have Wrong SourceInfo

### The Problem

Looking at `meta.rs:262-273`, when the recursive parse succeeds:

```rust
if pandoc.blocks.len() == 1 {
    if let crate::pandoc::Block::Paragraph(p) = &mut pandoc.blocks[0] {
        return MetaValueWithSourceInfo::MetaInlines {
            content: mem::take(&mut p.content),  // ⚠️ Inlines have wrong SourceInfo!
            source_info: source_info.clone(),     // ✓ Container has correct location
        };
    }
}
```

**The MetaValue container** gets the correct `source_info` (pointing to the YAML value location).

**BUT the individual nodes** inside `content` still have SourceInfo pointing to `<metadata>:1:X`!

### Example

YAML value:
```yaml
text: |
  Hello <em>world</em>
```

Result:
- `MetaInlines.source_info`: Points to line with `text:` (✓ correct)
- `MetaInlines.content`:
  - `[0]`: `Str("Hello")` with `source_info = Original(<metadata>, 0, 5)` (✗ wrong!)
  - `[1]`: `Space` with `source_info = Original(<metadata>, 5, 6)` (✗ wrong!)
  - `[2]`: `Emph([Str("world")])` with wrong nested SourceInfo (✗ wrong!)

### Impact

Any code that later:
1. Validates these nodes and creates diagnostics
2. Serializes and expects accurate source tracking
3. Uses these nodes' SourceInfo for error messages
4. Transforms the AST and needs to preserve locations

...will get **incorrect source locations**.

## Critical Downside #2: Multiple Location Fields in Diagnostics

From `diagnostic.rs`:
- `DiagnosticMessage.location` (line 186) - main location
- `DetailItem.location` (line 119) - additional locations per detail

We'd need to traverse and fix ALL location fields, not just one:

```rust
for mut warning in warnings {
    // Fix main location
    if let Some(loc) = &mut warning.location {
        *loc = adjust_source_info(loc, source_info);
    }

    // Fix ALL detail locations
    for detail in &mut warning.details {
        if let Some(loc) = &mut detail.location {
            *loc = adjust_source_info(loc, source_info);
        }
    }

    // Are there other location fields we're missing?
    diagnostics.add(warning);
}
```

**Risk**: Easy to miss a location field, leading to partial fixes.

## Critical Downside #3: Complex Offset Mapping

The recursive parse creates a **completely different SourceContext**:
- File: `<metadata>` (FileId in child context)
- Offsets: Relative to substring start

We can't just wrap the SourceInfo because **the FileIds don't match**!

The child diagnostic says: `Original(<metadata-FileId>, offset=10, ...)`
The parent context has: `<dj_index.qmd-FileId>`

We need to:
1. Extract offset from child's SourceInfo (which file? which context?)
2. Map it back through child's SourceContext
3. Create NEW SourceInfo as Substring of parent
4. Handle edge cases (Substring, Concat in child)

```rust
fn adjust_source_info(
    child_info: &SourceInfo,
    parent_info: &SourceInfo,
) -> SourceInfo {
    // How do we get the offset from child_info?
    // It might be Original, Substring, or Concat in a DIFFERENT context!

    // Can't use map_offset because it needs child's SourceContext,
    // which we don't have access to here!

    ???
}
```

This is **fundamentally difficult** because SourceInfo is tied to a SourceContext, and we're trying to map between two different contexts.

## Significant Downside #4: Lost Context Information

Using filename `<metadata>` loses information about:
- Original filename (`dj_index.qmd`)
- YAML path (`format.html.include-in-header.text`)

Better error messages would say:
> "In `format.html.include-in-header.text` of `dj_index.qmd` at line 12..."

But we've lost the YAML path information.

## Significant Downside #5: Fragile Pattern

The fix assumes:
- We can identify all location fields in diagnostics
- The diagnostic structure doesn't change
- No nested diagnostics or complex structures
- No additional metadata attached to locations

If the diagnostic structure evolves (new fields, nested structures), our fix might silently break.

## Moderate Downside #6: Semantic Confusion

We're **lying** about where content came from:
- Conceptually: Content is from a metadata value (distinct parse)
- After fix: Content appears to be from main file

This makes debugging harder:
- "Why does this diagnostic point to YAML?"
- "How did this content get parsed?"
- Stack traces won't make sense

## Moderate Downside #7: Testing Complexity

Tests checking diagnostic locations need to understand the transformation:

```rust
#[test]
fn test_metadata_warning() {
    // Warning is actually at offset 10 in YAML value string
    // But reported as offset 510 in main file (500 + 10)
    // Tests become harder to write and understand
}
```

## Moderate Downside #8: Future Maintenance

Future developers might:
- Add code that uses node SourceInfo directly (gets wrong locations)
- Create diagnostics from metadata nodes (wrong locations)
- Not realize SourceInfo is inconsistent between container and content

This creates a **footgun**: the API looks correct but has subtle bugs.

## Minor Downside #9: Performance

Creating new SourceInfo wrappers involves:
- Boxing (heap allocation)
- Cloning parent SourceInfo
- Multiple allocations per diagnostic

Probably negligible, but could add up with many metadata warnings.

## The Core Problem: Incomplete Fix

Option 1 is **fundamentally incomplete** because it only fixes diagnostics, not the AST.

### What Gets Fixed
- Warnings returned from initial parse ✓
- Their detail locations ✓ (if we remember to fix them)

### What DOESN'T Get Fixed
- Individual node SourceInfo in the AST ✗
- Future diagnostics created from those nodes ✗
- Serialized AST source tracking ✗
- Any code relying on node-level SourceInfo ✗

## Comparison: Would Options 2/3 Be Better?

### Option 2: Pass Parent Context to `read()`

**Pros**:
- Fixes EVERYTHING (AST + diagnostics)
- No post-processing needed
- Semantically correct
- Future-proof

**Cons**:
- Requires API change
- All call sites need updates
- More complex implementation

### Option 3: Reuse Parent SourceContext

**Pros**:
- Fixes AST nodes too
- Preserves context chain
- No API changes to `read()`

**Cons**:
- Still complex offset mapping
- Need to add substring to parent context
- State management complexity

## Recommendation

**Option 1 is risky** due to:
1. Incomplete fix (AST nodes wrong)
2. Complex offset mapping between contexts
3. Multiple location fields to fix
4. Future maintenance burden

**Unless** we can verify:
- No code uses metadata node SourceInfo after creation
- No future diagnostics created from metadata nodes
- Only parse-time diagnostics matter

If those hold, Option 1 is acceptable. Otherwise, Options 2 or 3 are safer.

## Questions to Answer

1. Does any code create diagnostics from MetaValue content nodes after parsing?
2. Do we serialize metadata with source tracking?
3. Are there validation passes that use node SourceInfo?
4. How often does metadata parsing generate warnings in practice?

These answers determine whether the incomplete fix is acceptable.
