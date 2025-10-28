# Fix: NoteDefinitionPara Support in Native Writer

**Date:** 2025-10-28
**Status:** In Progress

## Problem

The native writer (`-t native`) panics when encountering `NoteDefinitionPara` blocks:

```
thread 'main' panicked at crates/quarto-markdown-pandoc/src/writers/native.rs:594:14:
Unsupported block type in native writer: NoteDefinitionPara
```

## Background

### Quarto Markdown Note Definition Syntax

In QMD, footnotes can be defined using:
```markdown
[^1]: This is the footnote content.
```

This parses to a `NoteDefinitionPara` block with:
- `id`: "1"
- `content`: Inlines (the footnote text)

### Pandoc's Native Format

In Pandoc's native AST, footnotes don't exist as separate blocks. Instead, they appear inline where referenced:

```haskell
Para [Str "text", Note [Para [Str "footnote content"]], Str "."]
```

The note definition is coalesced into the Note inline element.

## Solution Options

### Option 1: Skip Note Definitions (Recommended)

Don't output `NoteDefinitionPara` blocks in native format at all, since Pandoc doesn't have an equivalent representation. The note content should already be attached to `Note` inline elements where they're referenced.

**Pros:**
- Matches Pandoc's semantic model
- Simple implementation
- Notes are already represented inline

**Cons:**
- Silent omission might be confusing

### Option 2: Output as Para

Convert to regular paragraph showing the note syntax:
```haskell
Para [Str "[^1]:", Space, ...content...]
```

**Pros:**
- Makes the note definition visible
- Shows structure explicitly

**Cons:**
- Not semantically correct for Pandoc
- Duplicates information (note content appears both inline and in definition)

### Option 3: Output Comment/Marker

Output some kind of marker or comment indicating a note definition existed:
```haskell
RawBlock "native" "-- Note definition: 1"
```

**Pros:**
- Indicates presence without duplication

**Cons:**
- Still not standard Pandoc
- Unclear value

## Recommendation

**Option 1**: Skip note definition blocks in native writer.

Rationale:
- Pandoc's native format doesn't have note definition blocks
- The actual note content should already be attached to Note inline elements
- QMD writer already properly handles roundtripping with `write_inlinerefdef`

## Implementation

Add a match arm in `write_block_native` before the panic:

```rust
Block::NoteDefinitionPara(_) | Block::NoteDefinitionFencedBlock(_) => {
    // Note definitions are not represented as separate blocks in Pandoc's native format.
    // The content is coalesced into Note inline elements where referenced.
    // Skip output for native writer.
}
```

Location: `crates/quarto-markdown-pandoc/src/writers/native.rs:594`

## Testing

1. Run on `external-sites/quarto-web/docs/authoring/article-layout.qmd`
2. Verify no panic
3. Check that output includes Note inline elements where footnotes are referenced
4. Verify QMD roundtrip still works correctly

## Related Code

- QMD writer: `write_inlinerefdef` (qmd.rs:588-596) - already handles note definitions correctly
- Block enum: `src/pandoc/block.rs:135-139` - defines NoteDefinitionPara
- Note inline element: Should contain the actual footnote content
