# HTML Comment Preservation in Incremental Writer

**Beads issue:** `bd-1066`
**Status:** Diagnosis complete, fix plan drafted

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

## Diagnostic Tests Added

File: `crates/pampa/tests/incremental_writer_tests.rs`

| Test | Status | What it verifies |
|------|--------|-----------------|
| `idempotent_with_standalone_comment` | PASS | Identity reconciliation preserves standalone comments |
| `comment_preserved_when_adjacent_block_changes` | PASS | Adjacent block change doesn't affect standalone comment block |
| `comment_lost_when_containing_paragraph_rewritten` | PASS | Inline comment dropped when paragraph is rewritten |
| `comment_inside_blockquote_lost_on_rewrite` | PASS | Comment in blockquote lost when blockquote is rewritten |
| `comment_block_lost_when_blocks_added` | PASS | Standalone comment survives block insertion (reconciler aligns empty Paras) |

All tests pass — they document the current (buggy) behavior. The "lost" tests assert that comments ARE lost, confirming the bug.

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

### Phase 2: Fix Design (TODO — needs user input)
- [ ] Decide on fix approach (A+B recommended, but user may prefer C or D)
- [ ] If A+B: Design the comment detection heuristic for the writer
- [ ] If C: Design the Comment AST node type and serialization format
- [ ] Consider impact on Lua filters, JSON serialization, WASM exports
- [ ] Consider interaction with the `\!` escaping in the reader

### Phase 3: Implementation (TODO)
- [ ] Implement chosen fix
- [ ] Convert diagnostic "comment_lost_*" tests to pass (comment preserved)
- [ ] Add round-trip tests: `<!-- comment -->` → parse → write → `<!-- comment -->`
- [ ] Test with incremental writer (inline comments survive paragraph rewrite)
- [ ] Test with standard writer (full write preserves comments)
- [ ] Full workspace test suite

### Phase 4: Edge Cases (TODO)
- [ ] Multi-line comments: `<!-- multi\nline\ncomment -->`
- [ ] Nested comment-like text: `<!-- <!-- nested --> -->`
- [ ] Comments adjacent to other constructs (lists, code blocks, divs)
- [ ] Comments in YAML front matter region
- [ ] Empty comments: `<!-- -->`
- [ ] Comments with special characters
