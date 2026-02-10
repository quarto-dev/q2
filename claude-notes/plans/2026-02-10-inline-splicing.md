# Phase 5: Inline Splicing for Incremental Writer

**Beads issue:** `bd-1hwd`
**Parent issue:** `bd-2t4o` (Incremental QMD Writer)
**Parent plan:** `claude-notes/plans/2026-02-07-incremental-writer.md`
**Status:** COMPLETE. All phases (5a-5g) done. 22 + 50 + 14 + 38 = 124 tests total for inline splicing.
**Branch:** `feature/inline-incremental-writer`

## Overview

The current incremental writer operates at block granularity: when any inline within a block changes, the entire block is rewritten. When that block is inside an indentation boundary (BlockQuote, BulletList, OrderedList), the **entire boundary** is rewritten. This means that changing a single word inside a bullet list rewrites every item in the list.

Phase 5 adds **inline-level splicing**: when a reconciliation plan indicates that inlines changed within a block, we can sometimes splice just the changed inlines without rewriting the enclosing block or indentation boundary. The key insight:

> If the reconciliation plan involves changes that neither remove nor add nodes that create newline characters (SoftBreak, LineBreak), then indentation boundaries are preserved, and an internal change of those inlines can safely avoid rewriting the entire indentation boundary.

### Why This Matters

The most common user edits are text changes within paragraphs: fixing a typo, toggling a checkbox, changing a word. When these paragraphs live inside lists or block quotes, the current conservative strategy rewrites the entire list/blockquote. In a collaborative editing context (hub-client), this generates unnecessarily large Automerge diffs and can cause merge conflicts.

### Safety Considerations

This is a data-integrity-critical feature. Bugs in the incremental writer mean **data loss** for users — their carefully formatted text could be silently corrupted. We must invest heavily in testing:

- Property-based tests covering all combinations of inline types and indentation contexts
- Exhaustive hand-crafted tests for edge cases
- Round-trip verification at every level

## The Core Invariant: No-Patch-Newlines Property

### The Problem: Indentation Context

Indentation boundaries (BlockQuote, BulletList, OrderedList) add line prefixes (`> `, `* `, `  `, etc.) after every newline in their content. In the current writer, these prefixes are injected by `BlockQuoteContext`/`BulletListContext` wrapper types that intercept all writes at the block level. When we splice inlines in isolation, we call `write_single_inline()` into a plain buffer **without** these context wrappers.

This means: **if the written replacement text for any inline patch contains a `\n` character, it will be missing the indentation prefix that should follow that newline.** The result would be malformed markdown — a line inside a block quote without its `> ` prefix would silently exit the block quote.

### Design Discussion: Why "Break Count Preserved" Is Insufficient

An earlier draft of this design proposed that inline splicing is safe when "the number and positions of SoftBreak/LineBreak nodes are preserved." This was refined through the following analysis:

**The position problem.** Consider `* Hello\n  world` where we replace `Str("Hello")` with `Str("Goodbye")`. The SoftBreak that follows shifts its byte offset by 2 characters. But that's fine — we keep it verbatim from the source, we don't rewrite it. "Position" in terms of byte offsets is irrelevant; what matters is which inlines we actually *write*.

**The indentation synthesis problem.** Even if the break count is preserved (same number of SoftBreak/LineBreak before and after), we may still need to *write* a break. Consider a `UseAfter` alignment for an `Emph` node whose subtree contains a `SoftBreak`. Writing that Emph produces `*Hello\nworld*` — the `\n` is there, but the indentation prefix (e.g., `> `) that should follow it is missing, because we're writing into a plain buffer without the indentation context wrappers. We would need to synthesize the correct indentation string, which requires knowing the full nesting context (how many levels of block quote, what list indentation width, etc.).

**The key realization.** The safety criterion is not about the *count* of breaks — it's about whether any inline we actually *write* (as a replacement) will produce a `\n` in its output. If no patch output contains a newline, indentation context is irrelevant and splicing is safe.

### The Testable Safety Predicate

An inline change is **safe to splice** if and only if:

> **No inline that we write (as opposed to keep verbatim) produces a newline character in its output.**

Formally:

```rust
/// Check if an inline reconciliation plan can be safely spliced without
/// indentation context. Safe iff every inline we'd actually write
/// (UseAfter or rewritten within RecurseIntoContainer) has a break-free subtree.
fn is_inline_splice_safe(
    new_inlines: &[Inline],
    plan: &InlineReconciliationPlan,
) -> bool {
    for (result_idx, alignment) in plan.inline_alignments.iter().enumerate() {
        match alignment {
            InlineAlignment::KeepBefore(_) => {
                // Preserved verbatim from original source — always safe.
                // The original bytes already contain correct indentation.
            }
            InlineAlignment::UseAfter(_) => {
                // We'll write this inline fresh into a plain buffer.
                // If its subtree contains SoftBreak/LineBreak, the written
                // output will contain \n without indentation prefixes.
                if inline_subtree_has_break(&new_inlines[result_idx]) {
                    return false;
                }
            }
            InlineAlignment::RecurseIntoContainer { .. } => {
                // We'll recursively patch this container's children.
                // Recurse: any child we write must also be break-free.
                if let Some(nested_plan) = plan.inline_container_plans.get(&result_idx) {
                    let children = inline_children(&new_inlines[result_idx]);
                    if !is_inline_splice_safe(children, nested_plan) {
                        return false;
                    }
                }
                // If no nested plan, the container is kept as-is (safe).
            }
        }
    }
    true
}

/// Returns true if the inline or any descendant is SoftBreak or LineBreak.
fn inline_subtree_has_break(inline: &Inline) -> bool {
    matches!(inline, Inline::SoftBreak(_) | Inline::LineBreak(_))
        || inline_children(inline)
            .iter()
            .any(|child| inline_subtree_has_break(child))
}
```

### Three Scenarios

**Scenario A — Zero breaks in the block (simplest, implement first):**
Neither original nor new inline sequences contain any SoftBreak or LineBreak. Every inline we write is guaranteed to produce newline-free output. This covers the vast majority of real edits: most paragraphs in lists don't span multiple lines.

**Scenario B — Breaks exist, all are KeepBefore (implement second):**
Breaks exist in both original and new, but every break is `KeepBefore` in the alignment plan — we never *write* a break, we preserve its original bytes verbatim. The gaps around the breaks (including indentation prefixes like `> `) are also preserved because they're not part of any patch. Additionally, no container inline being rewritten (UseAfter/RecurseIntoContainer) has a break in its subtree.

Example of Scenario B working:
```
> Hello
> world
```
Inlines: `[Str("Hello"), SoftBreak, Str("world")]`. Change `Str("Hello")` to `Str("Goodbye")`.
Plan: `[UseAfter(0), KeepBefore(1), KeepBefore(2)]`.
We only patch the `Str("Hello")` span → `"Goodbye"` (no newline in patch output). The SoftBreak's bytes (`\n`) and the gap after it (`> `) are preserved verbatim. Result: `Goodbye\n> world` — correct.

**Scenario C — Breaks being written (future, requires indentation context):**
A rewritten inline contains a SoftBreak/LineBreak in its subtree. Writing it produces `\n` without the correct indentation prefix. This requires passing indentation context to the inline writer. **Not in scope for Phase 5.**

### Implementation Strategy

Start with **Scenario A only** as the gate for Phase 5 initial implementation. It's dead simple, impossible to get wrong, and covers the most common real-world edits. Add Scenario B as a follow-up once the investigation phase confirms how inline spans and gaps behave around break nodes.

### Table Safety Analysis

Tables are either:
- **Pipe tables**: Support misaligned columns, so inline changes within cells are safe
- **List-table divs**: Cells live inside bullet lists. The inlines within a cell paragraph are inside an indentation boundary, so the No-Patch-Newlines Property applies

In both cases, the property holds: inline changes where no patch produces a newline are safe.

### Completeness: All Indentation Boundary Types

| Boundary Type | Line Prefix | Continuation Prefix | Inline Splice Safe? |
|---|---|---|---|
| BlockQuote | `> ` | `> ` | Yes, if no patch produces `\n` |
| BulletList (item) | `* ` | `  ` (2 spaces) | Yes, if no patch produces `\n` |
| OrderedList (item) | `N. ` | spaces (width of `N. `) | Yes, if no patch produces `\n` |

## Algorithm Design

### Overview

The algorithm extends the existing three-step pipeline (coarsen → assemble → edit-compute) with a finer-grained handling of `RecurseIntoContainer` blocks that have inline plans.

Currently, `RecurseIntoContainer` at the block level always produces a `Rewrite` coarsened entry. With inline splicing, we add a new path:

```
RecurseIntoContainer for a block with inline_plans
  → Check is_inline_splice_safe()
  → If safe: produce InlineSplice coarsened entry (verbatim copy + inline patches)
  → If unsafe: fall back to Rewrite (current behavior)
```

### Step 0: Detect Inline-Spliceable Blocks

A block alignment is inline-spliceable if:

1. It is `RecurseIntoContainer { before_idx, after_idx }` in the plan
2. The block is an inline-content block (Paragraph, Plain, Header)
3. The plan has an `inline_plans` entry for this alignment index
4. The inline plan satisfies `is_inline_splice_safe()`

For top-level blocks (not inside any indentation boundary), condition 4 could theoretically be relaxed — but for simplicity and safety, we apply it uniformly in the initial implementation.

### Step 1: Inline Coarsening

When a block passes the safety check, we produce an `InlineSplice` coarsened entry instead of `Rewrite`:

```rust
enum CoarsenedEntry {
    Verbatim { byte_range: Range<usize>, orig_idx: usize },
    Rewrite { new_idx: usize },
    /// New: splice inlines within a block without rewriting the entire block
    InlineSplice {
        /// Byte range of the original block in original_qmd
        block_byte_range: Range<usize>,
        /// Index in original_ast.blocks
        orig_idx: usize,
        /// Index in new_ast.blocks (for the new inlines)
        new_idx: usize,
        /// The inline-level patches to apply within the block
        inline_patches: Vec<InlinePatch>,
    },
}

/// A patch to apply to the inline content within a block's source span.
struct InlinePatch {
    /// Byte range within original_qmd to replace (absolute offsets)
    range: Range<usize>,
    /// Replacement text (from writing the new inline).
    /// Invariant: contains no '\n' characters (guaranteed by is_inline_splice_safe).
    replacement: String,
}
```

### Step 2: Computing Inline Patches

For each inline in the `InlineReconciliationPlan`:

- **`KeepBefore(orig_idx)`**: No patch needed — the original bytes are kept
- **`UseAfter(new_idx)`**: Create a patch replacing `original_inline_span` with `write_single_inline(new_inlines[new_idx])`. The safety check guarantees this output contains no `\n`.
- **`RecurseIntoContainer { before_idx, after_idx }`**: Recursively compute patches for the container's children. The container's delimiters (e.g., `*...*` for Emph) are preserved from the original source; only the changed children within are patched.

The tricky part is **computing the byte range for each original inline**. We need the source spans of individual inlines within the block. See Investigation Phase.

### Step 3: Inline Source Span Analysis

Each inline carries `source_info: SourceInfo` with `start_offset()` and `end_offset()`. Important subtleties:

1. **Gaps between inlines**: Consecutive inlines may have gaps (e.g., the `*` delimiters of emphasis are not part of any child inline's span).

2. **Container inline delimiters**: For `Emph { content: [Str("word")] }`, the `*` delimiters are part of the Emph's span but not the child Str's span. When splicing the child Str, the delimiters are preserved.

3. **Inside indentation boundaries**: The inline source spans may include embedded line prefixes from the enclosing indentation boundary. This needs empirical verification (see Investigation Phase).

### Step 4: Assembly with Inline Patches

For `InlineSplice` entries, assembly works as:

1. Start with the original block's byte range from `original_qmd`
2. Apply each `InlinePatch` in reverse order (to preserve byte offsets)
3. The result is the patched block text

This text is then inserted into the result string at the block's position, exactly like a `Verbatim` or `Rewrite` entry.

## Investigation Findings (Phase 5a — COMPLETE)

Tests: `crates/pampa/tests/inline_span_investigation.rs` (22 tests, all passing)

### Finding 1: SoftBreak Spans Include Indentation Prefixes

**SoftBreak absorbs the line prefix into its span.** This is the most important finding.

| Context | SoftBreak span | Content |
|---|---|---|
| Top-level paragraph | `[5, 6)` | `\n` (1 byte) |
| Inside `> ` BlockQuote | `[7, 10)` | `\n> ` (3 bytes) |
| Inside `* ` BulletList | `[7, 10)` | `\n  ` (3 bytes — continuation indent) |
| Inside `1. ` OrderedList | `[8, 12)` | `\n   ` (4 bytes — continuation indent) |
| Inside `> * ` (nested) | `[9, 14)` | `\n>   ` (5 bytes — both prefixes) |

**Gaps between consecutive inlines are zero-width.** There are no gap bytes between Str, Space, SoftBreak, etc. — they tile perfectly. The indentation prefix is part of the SoftBreak span, not a gap.

**Implication for Scenario B:** When a SoftBreak is `KeepBefore`, copying its span verbatim automatically preserves the correct indentation prefix. There are no gap bytes to manage. This confirms Scenario B is safe and straightforward.

### Finding 2: LineBreak Behaves Differently from SoftBreak

**LineBreak spans only the `\` backslash** (1 byte). The `\n` and any indentation prefix are in a **gap** after the LineBreak span.

| Context | LineBreak span | Gap after | Next Str starts at |
|---|---|---|---|
| Top-level paragraph | `[5, 6)` = `\` | `[6, 7)` = `\n` | `[7, ...)` |
| Inside BlockQuote | `[7, 8)` = `\` | `[8, 11)` = `\n> ` | `[11, ...)` |

**Implication:** When LineBreak is `KeepBefore`, the gap bytes (containing `\n` + prefix) are also preserved because our patches only touch specific inline spans and leave everything else untouched. This works correctly, but is a subtlety to keep in mind.

### Finding 3: Container Delimiters Live in Gaps Between Parent and Child Spans

Container inline spans **include** their delimiters. Child spans **exclude** the delimiters. The delimiters appear as gaps between parent start and first child start, and between last child end and parent end.

| Container | Span text | Opening gap | Child span | Closing gap |
|---|---|---|---|---|
| `Emph` | `*Hello*` `[0,7)` | `*` `[0,1)` | `Hello` `[1,6)` | `*` `[6,7)` |
| `Strong` | `**Hello**` `[0,9)` | `**` `[0,2)` | `Hello` `[2,7)` | `**` `[7,9)` |
| `Link` | `[Hello](url)` `[0,28)` | `[` `[0,1)` | `Hello` `[1,6)` | `](url)` `[6,28)` |
| `Code` | `` `code` `` `[3,10)` | includes delimiters | N/A (leaf) | N/A |

**Nested containers:** For `**_Hello_**`:
- Strong: `[0, 11)`, opening `**` `[0,2)`, closing `**` `[9,11)`
- Emph (child of Strong): `[2, 9)`, opening `_` `[2,3)`, closing `_` `[8,9)`
- Str (child of Emph): `[3, 8)` = `Hello`

**Implication for splicing:** We can replace a child Str span without touching any delimiter bytes. For `RecurseIntoContainer` on an Emph, we patch only the changed children; the `*...*` delimiters are preserved in the gaps. This works at arbitrary nesting depth.

### Finding 4: Space and Str Spans Are Precise

- `Space` spans exactly 1 byte (the space character)
- `Str` spans exactly the text content (no surrounding whitespace or delimiters)
- All top-level inlines tile perfectly with zero-width gaps (100% coverage of paragraph content before the trailing `\n`)

### Finding 5: Code Inline Includes Delimiters

`Code` span includes the backtick delimiters: `` `code` `` `[3, 10)`. Since Code is a leaf inline (no children to recurse into), it's always fully replaced when changed. This is fine.

### Summary: Splicing Is Feasible

The span structure is ideal for inline splicing:
1. **Leaf inlines** (Str, Space, Code) have precise spans — replace the span, everything else is preserved
2. **Container inlines** (Emph, Strong, Link) have delimiter gaps — replace children, delimiters are preserved
3. **SoftBreak includes prefix** — keeping it verbatim preserves indentation automatically
4. **LineBreak + gap includes prefix** — keeping both verbatim preserves indentation
5. **Zero-width gaps between siblings** — no gap bytes to manage for consecutive leaf inlines

Both Scenario A (zero breaks) and Scenario B (breaks exist, all KeepBefore) are confirmed to work correctly with the observed span structure.

## Properties for Property-Based Testing

### Property 6: Inline Round-Trip (extends Property 1)

```
∀ original_qmd, inline_change within a block:
  let original_ast = read(original_qmd)
  let new_ast = apply_inline_change(original_ast, inline_change)
  let plan = reconcile(original_ast, new_ast)
  let result_qmd = incremental_write(original_qmd, original_ast, new_ast, plan)
  read(result_qmd) ≡ new_ast  (structural equality)
```

### Property 7: Inline Idempotence (extends Property 2)

```
∀ original_qmd:
  let ast = read(original_qmd)
  let plan = reconcile(ast, ast)
  incremental_write(original_qmd, ast, ast, plan) = original_qmd  (byte-for-byte)
```

This should already hold (existing property), but we add tests with inline-level identity reconciliation to verify.

### Property 8: Inline Locality (extends Property 4)

```
∀ original_qmd, single_inline_change at block i, inline j:
  let edits = compute_incremental_edits(...)
  // Edits should be contained within block i's span (or the innermost
  // indentation boundary containing block i)
  // Specifically, blocks OTHER than i should not be affected
```

### Property 9: No-Patch-Newlines Invariant

```
∀ original_qmd, inline_change:
  if is_inline_splice_safe(new_inlines, plan):
    let result = incremental_write(...)
    read(result) ≡ new_ast  // still correct
    // AND every InlinePatch.replacement contains no '\n'
```

### Property 10: Inline Splicing Produces Same Result as Block Rewrite

```
∀ original_qmd, inline_change that passes is_inline_splice_safe:
  let result_splice = incremental_write_with_inline_splice(...)
  let result_rewrite = incremental_write_without_inline_splice(...)  // force Rewrite
  read(result_splice) ≡ read(result_rewrite)  // semantic equivalence
```

This property ensures that the optimization doesn't change correctness.

### Property-Based Test Generators Needed

**Inline mutation generators** (new for Phase 5):

```rust
enum InlineMutation {
    /// Change text content of a Str inline (most common case)
    ChangeStr { block_idx: usize, inline_idx: usize, new_text: String },
    /// Toggle emphasis on a stretch of inlines
    ToggleEmph { block_idx: usize, start: usize, end: usize },
    /// Change link target
    ChangeLinkTarget { block_idx: usize, inline_idx: usize, new_url: String },
    /// Add an inline (NOT SoftBreak/LineBreak — preserves safety)
    InsertInline { block_idx: usize, position: usize, inline: Inline },
    /// Remove a non-break inline
    RemoveInline { block_idx: usize, position: usize },
    /// Add a SoftBreak (violates safety — should force block rewrite)
    InsertSoftBreak { block_idx: usize, position: usize },
    /// Remove a SoftBreak (violates safety — should force block rewrite)
    RemoveSoftBreak { block_idx: usize, position: usize },
}
```

The generators should produce mutations at multiple nesting levels:
- Top-level paragraphs (always safe for Scenario A if no breaks)
- Paragraphs inside block quotes (safe only if is_inline_splice_safe)
- Paragraphs inside bullet lists (safe only if is_inline_splice_safe)
- Paragraphs inside nested structures (blockquote > list > paragraph)
- Headers (always leaf, same as paragraph for inline purposes)

## Work Items

### Phase 5a: Investigation (source span experiments) — COMPLETE
- [x] Write investigation tests for inline source spans within indentation boundaries
  - BlockQuote containing paragraph with multi-line content
  - BulletList containing paragraph with multi-line content
  - OrderedList containing paragraph with multi-line content
  - Nested blockquote > list > paragraph
- [x] Write investigation tests for inline span coverage and gaps
  - Paragraph with emphasis: span of Emph vs child Str vs gap
  - Paragraph with link: span of Link vs child inlines
  - Paragraph with code: span of Code inline
  - Paragraph with multiple container inlines
- [x] Write investigation tests for container inline delimiter handling
  - Can we splice child Str inside Emph without touching `*...*`? **YES**
  - What about nested containers: `**_Hello_**` → `**_World_**`? **YES — delimiters at each level are in gaps**
- [x] Document findings and update design — see "Investigation Findings" section above

### Phase 5b: Safety Check Implementation — COMPLETE
- [x] Implement `is_inline_splice_safe()` function
- [x] Implement `inline_subtree_has_break()` helper
- [x] Implement `inline_children()` helper (extracts children from container inlines, returns `&[]` for leaves)
- [x] Write unit tests for safety check (36 tests in `tests/inline_splice_safety_tests.rs`)
  - `inline_children()`: leaf nodes, Emph, Strong, nested containers
  - `inline_subtree_has_break()`: leaf Str/Space/Code (false), SoftBreak/LineBreak (true), Emph with/without breaks, nested deep
  - Safe cases: Str change only, all KeepBefore, all UseAfter no breaks, Emph text changed, Code change
  - Scenario B safe: breaks all kept, LineBreak kept, multiple breaks all kept, recurse into Emph with kept break
  - Unsafe cases: UseAfter SoftBreak/LineBreak, UseAfter Emph with SoftBreak, UseAfter nested break, recurse with written break
  - Mixed: one unsafe among many safe → entire plan unsafe
  - Edge cases: empty plan, single Str UseAfter, recurse with no nested plan, deeply nested safe/unsafe
- [ ] Property test: is_inline_splice_safe implies no patch output contains `\n` (deferred to Phase 5f)

### Phase 5c: Inline Source Span Utilities — COMPLETE
- [x] Implement `inline_source_info(inline: &Inline) -> &SourceInfo` helper
- [x] Implement `inline_source_span(inline: &Inline) -> Range<usize>` helper
- [x] Implement `write_single_inline()` in qmd.rs — public wrapper for context-free inline writing
- [x] Implement `write_inline_to_string()` in incremental.rs — convenience wrapper returning String with debug_assert for no newlines
- [x] Unit tests (14 new tests in `tests/inline_splice_safety_tests.rs`):
  - `inline_source_span`: Str, Space, Emph (includes delimiters), Emph child (excludes delimiters), SoftBreak in blockquote (includes prefix), zero-gap tiling
  - `write_inline_to_string`: Str, Space, Emph, Strong, Code, Emph with space, no-newline for leaves, no-newline for containers
- [x] Debug assertion in `write_inline_to_string` — asserts no `\n` in output

### Phase 5d+5e: Inline Coarsening, Assembly, and Integration Tests — COMPLETE

**Implementation note:** The actual implementation simplified the plan's design. Instead of storing `InlinePatch` structs and applying them during assembly, we compute the full spliced block text during coarsening and store it as a `block_text: String` in the `InlineSplice` variant. This is simpler, avoids reverse-order patch application, and integrates cleanly with `assemble()`.

- [x] Add `InlineSplice { block_text: String, orig_idx: usize }` variant to `CoarsenedEntry`
- [x] Extend `coarsen()` to detect inline-spliceable blocks and compute spliced text
  - Check for inline_plans in ReconciliationPlan
  - Check block has inlines (Paragraph/Plain/Header via `block_inlines()`)
  - Check `is_inline_splice_safe()` — falls back to Rewrite if unsafe
  - Compute spliced text via `assemble_inline_splice()`
- [x] Implement `assemble_inline_splice()` — replaces inline content region of block, preserves prefix/suffix
- [x] Implement `assemble_inline_content()` — walks inline alignments: KeepBefore → copy bytes, UseAfter → write_inline_to_string, RecurseIntoContainer → recurse
- [x] Implement `assemble_recursed_container()` — preserves container delimiters from original, recursively assembles children
- [x] Extend `assemble()` to handle `InlineSplice` entries (just returns `block_text.clone()`)
- [x] Update `compute_separator()` to handle `InlineSplice` alongside `Verbatim`
- [x] Update `coarsen()` signature to accept `original_qmd`, `new_ast`, return `Result`
- [x] Integration tests (14 tests in `tests/inline_splice_integration_tests.rs`):
  - `splice_str_change_in_paragraph` — simple Str replacement
  - `splice_str_change_preserves_surrounding_text` — middle word change
  - `splice_str_change_in_header` — preserves `## ` prefix
  - `splice_str_change_in_multiline_paragraph` — Scenario B (SoftBreak KeepBefore)
  - `splice_str_change_in_blockquote` — preserves `> ` prefix
  - `splice_str_change_in_multiline_blockquote` — Scenario B in blockquote
  - `splice_str_change_in_bulletlist` — preserves `* ` prefix
  - `splice_str_change_in_multiline_bulletlist` — Scenario B in list
  - `splice_preserves_other_blocks` — multi-block document, only affected block changes
  - `splice_uses_inline_splice_not_rewrite` — verifies RecurseIntoContainer (not UseAfter) is used
  - `splice_str_change_inside_emphasis` — preserves `*...*` delimiters
  - `splice_str_change_inside_strong` — preserves `**...**` delimiters
  - `splice_idempotent_simple_paragraph` — identity round-trip
  - `splice_idempotent_blockquote_multiline` — identity round-trip in blockquote

### Phase 5f: Comprehensive Property Tests — COMPLETE

38 tests in `tests/inline_splice_property_tests.rs`. Uses proptest for randomized testing.

**Implementation notes:**
- Used StrLocation-based modification (find Str inlines in AST, modify by location) rather than mutation enum generators. This is simpler and covers the key cases.
- Property 8 (locality) tests check that unchanged blocks are preserved verbatim in the result text, rather than checking individual edit ranges, because `compute_incremental_edits` currently produces whole-document edits (a future optimization).
- Proptest generators: `gen_inline_rich_doc()` produces multi-block documents with paragraphs, emphasis, strong, headers, blockquotes, and bullet lists.

- [x] Property 6: inline round-trip correctness — 7 hand-crafted + 4 proptest (paragraphs, emphasis, strong, blockquotes, bullet lists, multi-line, multi-change)
- [x] Property 7: inline idempotence — 5 hand-crafted + 1 proptest (emphasis, strong, code, mixed formatting, multiline blockquote with emphasis)
- [x] Property 8: inline locality — 3 hand-crafted + 1 proptest (unchanged blocks preserved verbatim)
- [x] Property 9: no-patch-newlines invariant — 3 hand-crafted (simple paragraph, blockquote, multiline blockquote with Scenario B)
- [x] Property 10: splice ≡ full writer — 5 hand-crafted + 3 proptest (paragraph, emphasis, strong, blockquote, multiblock, rich doc)
- [x] Stress test: deeply nested blockquote > bullet list
- [x] Stress test: 10-block document with single change → verify 1 edit
- [x] Stress test: 8-block document with first and last block changed
- [x] Stress test: document with all block types
- [x] Edit monotonicity: 1 proptest extending Property 5 with inline splicing

### Phase 5g: Integration — COMPLETE

- [x] Verify all existing tests still pass — 6394 tests, 0 failures
- [x] Check WASM export — no API change needed. `incremental_write` public signature unchanged; only internal `coarsen()` changed. WASM crate (`wasm-quarto-hub-client/src/lib.rs:2172`) calls `incremental_write` directly.
- [x] Update plan file with findings and design adjustments
- [x] Workspace builds cleanly (`cargo build --workspace`)

**Performance and correctness discussion:**

The inline splicing optimization improves the *correctness* of the assembled result string: instead of rewriting an entire indentation boundary (list/blockquote) when a single word changes, we now patch only the changed inline spans, preserving all original formatting verbatim. This is the primary goal of Phase 5 — it ensures the incremental writer produces the smallest possible semantic diff for inline-level changes, which directly improves the collaborative editing experience.

For the current hub-client use case — one user in Monaco (direct text editing) and another in an HTML app with the incremental writer mediating — the path is:

1. HTML app modifies the AST (e.g., toggles a checkbox, changes a word)
2. `incremental_write` produces a new QMD string with minimal changes
3. Automerge diffs the old and new strings to produce CRDT operations
4. Monaco receives and applies those operations

Step 2 now produces a result that differs from the original in only the changed inline spans (thanks to inline splicing). Step 3 is O(|document|) because `incremental_write` returns a full string and Automerge must diff it. For small documents, this O(|document|) cost is acceptable and the collaborative editing experience is good: Automerge's diff will find a small, localized change and produce correspondingly small CRDT operations.

**Future optimization: fine-grained edit ranges.** The `compute_incremental_edits` function currently produces a single whole-document `TextEdit` (replacing `0..len` with the full result string). The coarsened plan already knows the precise byte ranges that changed — each `InlineSplice` entry was computed from exact inline source spans. A future improvement could walk the coarsened entries and emit one `TextEdit` per changed span, e.g., `{ range: 2..7, replacement: "Goodbye" }`. This would allow the caller to apply edits directly to an Automerge `Text` object without a full string diff, reducing the per-edit cost from O(|document|) to O(|edit|). This matters for large documents where the full-string diff becomes a bottleneck, but is not urgent for the current small-document use case.

## Design Alternatives Considered

### Alternative A: Rewrite Inlines Within Block (No Span Splicing)

Instead of patching individual inline spans, rewrite the entire inline content of the block but keep the block's structural framing (e.g., `## ` prefix for headers, `> ` for blockquotes).

**Pro**: Simpler — no need for inline source span analysis.
**Con**: Still requires understanding the block's structural framing to produce correct output. Also doesn't provide true inline-level incrementality.

**Verdict**: This is a halfway measure. If we're going to the effort of detecting inline-safe changes, we should get the full benefit of inline-level splicing.

### Alternative B: Line-Level Splicing

Instead of inline-level splicing, splice at the level of whole lines within a block. If a change affects only inlines on line 3 of a paragraph inside a blockquote, rewrite only line 3 (with its `> ` prefix).

**Pro**: Simpler than inline-level splicing. Handles the indentation prefix problem naturally.
**Con**: Less granular. A change to one word still rewrites the entire line. Also requires tracking which inlines are on which line, which is non-trivial.

**Verdict**: Could be a useful intermediate step, but inline-level splicing is more general and the infrastructure we're building supports it directly.

### Alternative C: Two-Phase Approach

Phase 5c implements "block-level inline rewrite" (Alternative A), Phase 5d adds full inline splicing.

**Verdict**: The safety check is the key gate. Once we have that, the jump from "rewrite block inlines" to "splice individual inlines" is small. Better to build the right thing once.

### Alternative D: Pass Indentation Context to Inline Writer

Extend `write_single_inline()` to accept indentation context (nesting depth, prefix strings), so it can correctly emit prefixes after newlines.

**Pro**: Would allow splicing even when breaks change (Scenario C).
**Con**: Significant complexity — need to reconstruct the full indentation context from the AST nesting structure, and the current writer architecture doesn't support this (indentation is handled by Write trait wrappers, not by parameters).

**Verdict**: Out of scope for Phase 5. The no-patch-newlines approach avoids this entirely. Can revisit if Scenario C becomes important.

## References

- `crates/pampa/src/writers/incremental.rs` — current incremental writer
- `crates/pampa/src/writers/qmd.rs` — QMD writer (write_inline, write_block, indentation contexts)
- `crates/quarto-ast-reconcile/src/types.rs` — InlineReconciliationPlan, InlineAlignment
- `crates/quarto-ast-reconcile/src/compute.rs` — compute_inline_alignments
- `crates/pampa/tests/incremental_writer_tests.rs` — existing property tests
- `crates/pampa/tests/incremental_writer_investigation.rs` — source span investigation tests
- `claude-notes/plans/2026-02-07-incremental-writer.md` — parent plan
