# HTML Comment Preservation in Incremental Writer

**Beads issue:** `bd-1066`
**Status:** Phases 1-3 complete, Phase 4 edge cases remain

## Problem Summary

HTML comments (`<!-- ... -->`) are lost during incremental writes when the containing block is rewritten. The root cause is that comments are never represented in the Pandoc AST — they are parsed by tree-sitter as `comment` nodes, converted to `IntermediateUnknown` in the treesitter-to-pandoc conversion, and silently dropped.

## Root Cause Chain

```
QMD source: "Hello <!-- comment --> world."
  ↓ tree-sitter inline parser
  comment node (0,3)-(0,19)          ← recognized as a comment
  ↓ treesitter.rs:1032
  IntermediateUnknown(range)          ← treated as unknown/error
  ↓ document.rs / inline collection
  (silently dropped)                  ← not included in AST
  ↓ Pandoc AST
  Para([Str("Hello"), Space, Str("world.")])  ← no comment
```

## Impact Analysis

### What survives (KeepBefore / verbatim copy)

1. **Standalone block-level comments** (`<!-- ... -->` on own line): Parsed as a `pandoc_paragraph` containing only a `comment` child. The comment is dropped, producing an **empty Para** block. The empty Para's source span covers the comment text. When the reconciler marks this as `KeepBefore`, the original text (including the comment) is copied verbatim from the source span.

2. **Comments in gaps between consecutive verbatim blocks**: The assembly step copies `original_qmd[prev_span.end..curr_span.start]` between consecutive KeepBefore blocks, which includes any comments in the gap.

3. **Comments in the metadata prefix region**: The `emit_metadata_prefix` function copies `original_qmd[..first_block_start]` verbatim when metadata is unchanged.

### What is lost (Rewrite)

1. **Inline comments** (`Hello <!-- comment --> world.`): The comment is completely absent from the Para's inline list. When the paragraph is rewritten (e.g., because any text in it changed), the comment disappears.

2. **Standalone comments in rewritten regions**: If an empty Para block (containing a comment via source span) is marked as `UseAfter` → `Rewrite`, the writer produces an empty paragraph and the comment is lost.

3. **Comments inside indentation boundaries** (blockquote, list items): When the boundary block is rewritten (because any child changed), the entire block is re-emitted from the AST, which has no comment.

4. **Comments near structural changes**: If blocks are added/removed around a comment, the reconciler might fail to align the empty Para, marking it as `UseAfter` → `Rewrite`.

## Tests

File: `crates/pampa/tests/incremental_writer_tests.rs`

12 tests verify comment preservation after the fix:

| Test | What it verifies |
|------|-----------------|
| `idempotent_with_standalone_comment` | Identity reconciliation preserves standalone comments |
| `idempotent_with_inline_comment` | Identity reconciliation preserves inline comments |
| `idempotent_with_comment_in_blockquote` | Identity reconciliation preserves comments in blockquotes |
| `idempotent_with_multiple_comments` | Multiple comments in one paragraph survive |
| `idempotent_with_edge_position_comments` | Comments at start/end of document survive |
| `comment_preserved_when_adjacent_block_changes` | Adjacent block change doesn't affect standalone comment block |
| `inline_comment_preserved_when_paragraph_rewritten` | Inline comment survives paragraph rewrite |
| `comment_inside_blockquote_preserved_on_rewrite` | Comment in blockquote survives blockquote rewrite |
| `comment_block_preserved_when_blocks_added` | Standalone comment survives block insertion |
| `roundtrip_inline_comment` | Standard write round-trips inline comments |
| `roundtrip_standalone_comment` | Standard write round-trips standalone comments |
| `roundtrip_comment_in_blockquote` | Standard write round-trips comments in blockquotes |

Additionally, 45 JSON snapshot tests were updated to reflect the new `RawInline` output for comments.

## Key Code Locations

| File | Line | What |
|------|------|------|
| `crates/pampa/src/pandoc/treesitter.rs` | 1032-1035 | `"comment" → IntermediateUnknown` — the root cause |
| `crates/pampa/src/pandoc/treesitter_utils/document.rs` | 38-41 | Block-level `IntermediateUnknown` silently skipped |
| `crates/pampa/src/writers/incremental.rs` | 170-178 | Block text emission (Verbatim vs Rewrite) |
| `crates/pampa/src/writers/incremental.rs` | 270-296 | Gap/separator computation between blocks |
| `crates/pampa/src/writers/qmd.rs` | 1612-1623 | `write_rawinline` — would handle comments IF they were in the AST |

## Possible Fix Approaches

### Approach A: Preserve comments in the AST as RawInline(html)

Change `treesitter.rs:1032` to convert `comment` nodes into `Inline::RawInline(RawInline { format: "html", text: comment_text })` instead of `IntermediateUnknown`. This is exactly what happens for `html_element` nodes (line 1036).

**Pros:**
- Simple change at the root cause
- Comments round-trip through the AST naturally
- The QMD writer already handles RawInline (backtick syntax), though this changes the comment format

**Cons:**
- Changes comment format on write: `<!-- comment -->` becomes `` `<!-- comment -->`{=html} ``
- Requires updating the QMD writer to emit native `<!-- ... -->` syntax for RawInline nodes that contain HTML comments
- Affects ALL reads, not just incremental writes — could change behavior of other tools

### Approach B: Teach the QMD writer to emit HTML comments natively

In addition to Approach A, modify `write_rawinline` to detect when the text is an HTML comment (`<!-- ... -->`) and emit it directly instead of using backtick syntax.

**Pros:**
- Comments round-trip perfectly: `<!-- ... -->` → RawInline → `<!-- ... -->`
- No format change visible to users

**Cons:**
- Slightly more complex writer logic
- Need to handle the `\!` escaping that the reader applies (the reader escapes `!` in raw content)

### Approach C: Preserve comments as a dedicated AST node

Add a `Comment` block/inline type to the Pandoc AST (`quarto-pandoc-types`). This is the most principled approach.

**Pros:**
- Comments are first-class citizens in the AST
- No confusion with RawInline
- Writer can emit native comment syntax directly

**Cons:**
- Largest change: new AST node type, serialization, all consumers must handle it
- Need both Block::Comment and Inline::Comment variants
- Must update JSON serialization, Lua filter API, WASM exports, etc.

### Approach D: Source-span-aware incremental writer (workaround)

Don't fix the AST. Instead, modify the incremental writer to check for "hidden content" in the gaps between a block's inline source spans and the block's overall source span. When rewriting a block, splice the original comment text from these gaps.

**Pros:**
- No change to parsing or AST types
- Only affects the incremental writer

**Cons:**
- Complex implementation: must track which byte ranges within a block are "AST-invisible"
- Fragile: depends on exact source span alignment
- Only fixes incremental writes, not the standard writer
- Comments still lost in full writes (`write()`)

### Recommended approach: A + B (combined)

1. Convert `comment` → `RawInline(html, text)` in the parser (Approach A)
2. Teach the writer to emit `<!-- ... -->` for HTML comment RawInlines (Approach B)
3. This provides correct round-tripping for both the standard and incremental writer

## Work Items

### Phase 1: Diagnosis (COMPLETE)
- [x] Investigate how comments are parsed (tree-sitter `comment` → `IntermediateUnknown` → dropped)
- [x] Trace the full pipeline: parser → treesitter_to_pandoc → postprocess → AST
- [x] Write diagnostic tests documenting current behavior
- [x] Verify workspace tests pass with diagnostic tests
- [x] Analyze which scenarios preserve vs lose comments
- [x] Document key code locations
- [x] Create beads issue (bd-1066)

### Phase 2: Fix Design (COMPLETE — Approach A+B chosen)
- [x] Decide on fix approach: A+B (parser preserves + writer emits native syntax)
- [x] Design the comment detection heuristic for the writer (`is_html_comment`)
- [x] ~~Confirmed no impact on `\!` escaping~~ **WRONG**: inline parser produces `html_element` (not `comment`), and the `html_element` handler escapes `!` → `\!`. See Phase 4 notes.

### Phase 3: Implementation (COMPLETE)
- [x] Implement parser fix: `comment` → `RawInline(html, text)` in `treesitter.rs:1032`
- [x] Implement writer fix: `write_rawinline` emits native `<!-- -->` for HTML comments in `qmd.rs`
- [x] Convert diagnostic tests to verify comment preservation (12 tests)
- [x] Test with incremental writer (inline comments survive paragraph rewrite)
- [x] Test with standard writer (full write preserves comments)
- [x] Update 45 JSON snapshot tests
- [x] Full workspace test suite: 6262 tests pass, 0 failures

### Phase 4: Edge Cases (IN PROGRESS)

**Important discovery:** The inline tree-sitter grammar classifies `<!-- -->` as
`html_element`, NOT `comment`. The `comment` node type only appears in some
contexts (the snapshot test fixtures use it). The `html_element` handler applies
`\!` escaping to the text, producing `<\!-- ... -->` in the RawInline text.
This means:

1. The `is_html_comment` check in `write_rawinline` must also handle escaped text
   (`<\!--` in addition to `<!--`).
2. The Phase 2 note "Confirmed no impact on `\!` escaping" was **wrong** — the
   escaping does affect the round-trip path through the standard writer.
3. The incremental writer tests pass because they test idempotence (KeepBefore
   copies verbatim) and the rewrite path happens to work for the specific test
   cases — but a full `parse → standard_write → parse` cycle through the binary
   does NOT preserve native comment syntax.

**Next steps:**
- [ ] Fix `is_html_comment` to handle `<\!--` escaped form
- [ ] Investigate whether `html_element` handler should detect comments and skip `\!` escaping
- [ ] Multi-line comments: `<!-- multi\nline\ncomment -->`
- [ ] Nested comment-like text: `<!-- <!-- nested --> -->`
- [ ] Comments adjacent to other constructs (lists, code blocks, divs)
- [ ] Comments in YAML front matter region
- [ ] Empty comments: `<!-- -->`
- [ ] Comments with special characters
