# Incremental QMD Writer

**Beads issue:** `bd-2t4o`
**Parent plan:** `claude-notes/plans/2026-02-06-ast-sync-client-api.md` (Phase 2)
**Status:** Phases 0-4 COMPLETE — all property tests passing, WASM export + sync client integration done
**Branch:** `feature/incremental-writer`

## Resumption Notes

When continuing this work in a new session, read this plan file first. Key context:

1. **Phase 0 is COMPLETE.** All design items are resolved and documented.
2. **Phase 1 is MOSTLY COMPLETE.** Module created, public wrappers added, investigation tests done. Proptest infrastructure not yet set up.
3. **Phase 2 core implementation is DONE.** `incremental_write()` and `compute_incremental_edits()` are implemented and passing 28 hand-crafted tests. Two bugs were found and fixed in `emit_metadata_prefix()` and separator logic.
4. **Source span experiments are done** (see "Source Span Experimental Findings" section). 20 tests in `crates/pampa/tests/incremental_writer_investigation.rs` document the span behavior.
5. **Sugar/desugar coupling comments added** to `postprocess.rs` (transform registry at top of file) and `qmd.rs` (write_table, write_list_table, write_definitionlist).
6. **Key files:**
   - `crates/pampa/src/writers/incremental.rs` — core incremental writer (types, coarsening, assembly, edit computation)
   - `crates/pampa/src/writers/qmd.rs` — added `write_single_block` and `write_metadata` public wrappers
   - `crates/pampa/tests/incremental_writer_tests.rs` — 28 tests (17 idempotence, 8 round-trip, 1 verbatim preservation, 2 edge cases)
   - `crates/pampa/tests/incremental_writer_investigation.rs` — 20 source span investigation tests
7. **Key architectural decisions:**
   - Module lives in `pampa` as `writers::incremental` (Q5)
   - Reconcile post-desugared ASTs — Option A (Q7)
   - Structural equality ignoring source info for round-trip property (Q6)
   - Metadata: no incremental handling, full rewrite on change (Q3)
   - Format canonicalization on `Rewrite` is acceptable (Q8)
   - API: both `incremental_write` and `compute_incremental_edits`
   - `write_block` access: public `write_single_block` wrapper (Option 2)
8. **Key design details (see Algorithm Design section):**
   - 3 indentation boundaries (BlockQuote, BulletList, OrderedList), 3 non-boundary containers (Div, Figure, NoteDefinitionFencedBlock), 2 always-rewrite (Table, DefinitionList), 11 leaf blocks
   - Coarsening: KeepBefore→Verbatim, UseAfter/RecurseIntoContainer→Rewrite (conservative)
   - Assembly: copy verbatim spans, use original gaps for consecutive verbatim blocks, `\n` separator otherwise
   - BulletList quirk: span ends with `\n\n`, `compute_separator` checks `ends_with("\n\n")` to avoid extra separators
9. **Discovered issue:** DefinitionList sugar/desugar is LOSSY — writer produces Pandoc-native `:` syntax, but reader only recognizes `::: {.definition-list}` div syntax. This means Rewrite of a DefinitionList block won't round-trip correctly. KeepBefore (verbatim copy) works fine. Tests document this with `#[ignore]`.
10. **Test counts:** 47 passing tests + 2 ignored (definition-list roundtrip)
    - 28 hand-crafted (17 idempotence, 8 roundtrip, 1 verbatim, 2 edge cases)
    - 12 proptests (4 idempotence levels, 4 roundtrip mutations, 1 equivalence, 1 verbatim, 2 monotonicity)
    - 7 sugar/desugar tests (2 list-table roundtrip, 3 idempotence, 2 incremental roundtrip near sugared blocks)
    - 2 ignored (definition-list roundtrip — pre-existing writer bug)
11. **Phase 4 integration is DONE:**
    - WASM export: `incremental_write_qmd(original_qmd, new_ast_json)` in `wasm-quarto-hub-client/src/lib.rs` — re-parses `original_qmd` internally for guaranteed correct source spans, deserializes `new_ast_json`, computes reconciliation, calls incremental writer
    - Sync client: `ASTOptions.incrementalWriteQmd` optional field in `quarto-sync-client/src/types.ts`, used in `updateFileAst` when available with cached source
    - Demo app: `hub-react-todo/src/wasm.ts` wrapper + wired into `useSyncedAst.ts`
    - TypeScript type declarations updated in `hub-client/src/types/wasm-quarto-hub-client.d.ts`
    - New dependency: `quarto-ast-reconcile` added to `wasm-quarto-hub-client/Cargo.toml`
12. **Next steps:** End-to-end verification (requires WASM build via `npm run build:all`), then Phase 5 (future: inline splicing)

## Overview

Design and implement an incremental writer for pampa that converts localized AST changes into localized string edits. When a change occurs in the AST, only the affected portion of the QMD string should be rewritten, preserving the rest of the original source text verbatim.

### Motivation

The current `writers::qmd::write()` always rewrites the entire document from the AST. This means that even toggling a single checkbox in a todo list reformats the entire QMD string — destroying user formatting, whitespace choices, and creating unnecessarily large diffs in the Automerge sync layer.

The incremental writer enables the hub-client's `updateFileAst()` to produce minimal text edits.

## Background: The Fundamental Properties

### The Round-Tripping Property (existing writer)

A correct writer satisfies:

```
read(write(ast)) = ast
```

where equality ignores source location trivia but preserves semantic content.

### The Incremental Round-Tripping Property

```
read(incremental_write(original_qmd, original_ast, new_ast)) = new_ast
```

### The Incrementality Property (the hard part)

The above is necessary but not sufficient. A trivial implementation satisfies it:

```
bad_incremental_write(_, _, ast) = write(ast)
```

We need a property that captures "unchanged parts of the AST produce unchanged parts of the string."

## The Core Design Challenge: Indentation-Sensitive Constructs

### Why Markdown Isn't Like Lisp

In a parenthesized language, if expression A contains expression B, then `span(A) ⊃ span(B)` — the source span of the container fully contains the source span of the contained. This means you can splice at any level: to rewrite B, just replace `span(B)` in the string.

Markdown breaks this property. Consider:

```
> > A paragraph in a block quote.
> > Continued here.
```

The source information for the inner paragraph is _not_ a contiguous span — the `> > ` prefixes on line 2 are interleaved with the paragraph content. You cannot simply call `write(paragraph)` and splice the result into the document, because the result would be missing the `> > ` line prefixes.

### Indentation Boundary Nodes

We define **indentation boundary nodes** as AST node types whose children's source representations are non-contiguous due to indentation/prefix requirements. Specifically, inner content source spans include line prefixes from the container, making them impossible to splice independently.

| Node Type | Prefix/Indentation | Boundary? |
|---|---|---|
| `BlockQuote` | `> ` prefix on every continuation line | **YES** |
| `BulletList` (item) | `* ` on first line, `  ` on continuations | **YES** |
| `OrderedList` (item) | `N. ` on first line, spaces on continuations | **YES** |

### Complete Block Type Classification

**Indentation boundaries** — inner block source spans are non-contiguous (include line prefixes from the container). Splicing inner blocks is not possible; the entire boundary must be rewritten if any inner block changes.

| Block Type | Why |
|---|---|
| `BlockQuote` | `> ` prefix on every continuation line (confirmed by experiments) |
| `BulletList` | `* `/`  ` indent per item (confirmed by experiments) |
| `OrderedList` | `N. `/spaces indent per item (confirmed by experiments) |

**Non-boundary containers** — contain nested blocks, but inner content source spans are contiguous (no per-line prefix). The `:::` fences are separate from the content. Inner blocks can potentially be spliced independently.

| Block Type | Why |
|---|---|
| `Div` | `:::` fences, content lines NOT prefixed (confirmed by experiments) |
| `Figure` | `:::` fences (same as Div), content lines NOT prefixed |
| `NoteDefinitionFencedBlock` | `:::` fences, content lines NOT prefixed |

**Always-rewrite blocks** — complex structure or desugared representation makes incremental splicing impractical. Always fully rewritten when changed, regardless of indentation boundary classification.

| Block Type | Why |
|---|---|
| `Table` | Complex cell structure; desugared from list-table or pipe table |
| `DefinitionList` | Desugared from definition-list div |

**Leaf blocks** — no nested blocks. Either contain only inlines (contiguous span) or simple content. Rewritten as a unit when changed.

| Block Type | Content |
|---|---|
| `Paragraph` | Inlines (contiguous span) |
| `Plain` | Inlines (contiguous span) |
| `Header` | Inlines (contiguous span) |
| `CodeBlock` | Text content (no nested structure) |
| `RawBlock` | Text content (no nested structure) |
| `HorizontalRule` | No content |
| `BlockMetadata` | Metadata value |
| `NoteDefinitionPara` | `[^id]: ` prefix + inlines (single logical line) |
| `CaptionBlock` | Inlines (should not appear in final output — postprocessing artifact) |
| `LineBlock` | `\| ` prefix per line + inlines (Phase 5: inline-level indentation boundary) |
| `Custom` | Not rendered in QMD output |

**Key insight for Phase 1 (conservative):** In the conservative strategy, ALL container blocks (both indentation boundaries and non-boundaries) are fully rewritten if any inner block changes. The distinction between boundaries and non-boundaries only matters for later optimization phases where we'd want to recursively splice inner blocks of non-boundary containers like Div.

**Key insight for Phase 5 (inline splicing):** `LineBlock` would become an indentation boundary at the inline level, since each line has a `| ` prefix. But since LineBlock contains `Vec<Inlines>` (not nested blocks), this is irrelevant for block-level coarsening.

### The "Safe Rewrite Boundary" Rule

When a reconciliation plan indicates that an inner node of an indentation boundary node has changed, the incremental writer must **rewrite the entire indentation boundary node** rather than attempting to splice the inner change.

More precisely, define `safe_rewrite_ancestor(node)` as:
- If `node` is not inside any indentation boundary: `node` itself
- Otherwise: the innermost indentation boundary ancestor of `node`

The incremental writer rewrites at the granularity of `safe_rewrite_ancestor`.

## Interaction with Syntactic Sugar (Desugaring Pipeline)

### The Problem

Pampa has a multi-stage processing pipeline where certain QMD constructs are "desugared" on read and "sugared" on write. The incremental writer must account for this.

#### Current Sugar/Desugar Transforms

| Transform | Read (desugar) | Write (sugar) | Location |
|---|---|---|---|
| **List-table** | Div with `.list-table` → `Table` | `Table` → Div with `.list-table` (if complex) or pipe table (if simple) | `postprocess.rs` / `qmd.rs` |
| **Definition-list** | Div with `.definition-list` → `DefinitionList` | `DefinitionList` → Pandoc definition-list syntax | `postprocess.rs` / `qmd.rs` |

There are also non-sugar transforms applied during postprocessing (auto-ID generation for headers, citation numbering, string merging), but these don't create the same structural mismatch.

#### The Pipeline Stages

```
QMD text
  ↓ tree-sitter parse
  ↓ treesitter_to_pandoc()
Pre-desugared AST  ← Divs with .list-table, .definition-list; source spans point to original QMD
  ↓ transform_divs() filter
Post-desugared AST ← Tables, DefinitionLists; source spans still point to original QMD
  ↓ (application logic works here)
Modified AST       ← Tables, DefinitionLists (from app edits)
  ↓ write_table() / write_definitionlist() (sugaring)
  ↓ write_block() / write_inline()
QMD text
```

### The Snapshot Question

The reconciliation plan must compare two ASTs that are structurally close to the actual QMD text. This means we need to reconcile at the right pipeline stage.

**Option A: Reconcile post-desugared ASTs (both have Tables)**

This is the natural choice — the application works with Tables, so both `original_ast` and `new_ast` are post-desugared. The source spans in `original_ast` still point to the original QMD text (the list-table div syntax), so `KeepBefore` blocks can be spliced verbatim.

For `Rewrite` blocks, the standard writer already knows how to sugar Tables back to list-table divs, so this works correctly.

**Option B: Reconcile at pre-desugared level**

Reconcile the pre-desugared `original_ast` (has Divs) against a re-sugared `new_ast` (Tables converted back to Divs). This gives structurally closer ASTs for reconciliation, but adds complexity: we'd need to sugar the `new_ast` before reconciling, and the re-sugared Divs might not perfectly match the original Div structure.

**Decision**: Option A is simpler and sufficient for the initial implementation. The key observation below explains why:

### Why This Doesn't Block Incrementality (For Now)

The sugared forms of both list-tables and definition-lists use **bullet lists** as their textual representation. Bullet lists are indentation boundary nodes. This means:

- A `Table` node in the post-desugared AST, when its source span points to a list-table div in the original QMD, will be written as a list-table div containing bullet lists.
- Since bullet lists are indentation boundaries, the entire Table/list-table must be rewritten if any cell changes — we can never incrementally splice into the middle of a list-table.
- The same applies to definition-lists.

**Consequence**: Tables and definition-lists will always be fully rewritten by the incremental writer. This is acceptable because the safe-rewrite-boundary rule already requires this.

### Correctness Requirement

Even though tables and definition-lists are always fully rewritten, the incremental writer must still produce correct output when these constructs are present:

1. **Round-trip**: A `KeepBefore` Table must be faithfully reproduced from its source span in `original_qmd`.
2. **Rewrite**: A changed Table must be correctly sugared (by the standard writer) and the result must parse back to the same Table.
3. **Mixed documents**: Documents containing both simple blocks (paragraphs, headers) and sugared constructs (tables, definition-lists) must work correctly — simple blocks spliced from original, sugared constructs fully rewritten.

### Future Consideration

If we ever want incremental updates _inside_ tables or definition-lists, we would need to:
1. Work at the pre-desugared AST level (Option B above)
2. Define a finer-grained notion of "safe splicing" for the internal structure of list-table divs
3. This is not needed for the initial implementation

## Source Span Experimental Findings

Tests in `crates/pampa/tests/incremental_writer_investigation.rs` — 20 tests, all passing.

### Key Finding: Block Spans Are "Content + Trailing Newline"

Each block's source span covers its content text plus one trailing `\n`. Block separators (the blank lines between blocks) fall in **1-byte gaps** between consecutive spans.

**Pattern for simple sequential blocks:**
```
Input:  "## Title\n\nFirst paragraph.\n\nSecond paragraph.\n"
         [0,  9)    [10,  27)         [28,  46)
         ^^^^^^^^   ^^^^^^^^^^^^^^^^^  ^^^^^^^^^^^^^^^^^^
         Header\n   First paragraph.\n Second paragraph.\n
              gap=[9,10)="\n"   gap=[27,28)="\n"
```

Each block span includes its own trailing `\n` but NOT the blank-line separator. The gaps are consistently single `\n` characters. **Coverage is ~95%** of the input (the remaining 5% is the inter-block `\n` separators).

### Assembly Strategy for Incremental Writer

For `Verbatim` blocks, copy `original_qmd[start..end]` which gives the block text + trailing newline. Between blocks, insert the gap (typically `\n` for a blank line separator). This means:

1. For a `Verbatim` block: copy its span verbatim
2. Between any two blocks: insert `\n` (the separator)
3. For a `Rewrite` block: call the standard writer (which will produce text ending in `\n`)

This is clean and simple. The only concern is whether the separator is always `\n` or if it can vary — the experiments show it's consistently `\n` for all tested cases.

### Detailed Findings by Block Type

**Leaf blocks (no nesting concerns):**
- `Paragraph`: `[start, end)` = content text + `\n`
- `Header`: `[start, end)` = `## Title\n` (includes `##` prefix)
- `CodeBlock`: `[start, end)` = ` ```python\n...\n```\n ` (includes fences)
- `HorizontalRule`: `[start, end)` = `***\n`

**Container blocks — indentation boundaries confirmed:**
- `BlockQuote`: Outer span covers everything `> ...\n> ...\n`.
  - **Inner paragraph spans include the `> ` prefix from continuation lines!** e.g., `"Quoted paragraph.\n> Continued.\n"` — the inner Para span is `[11, 42)` which includes the `> ` of line 2. This confirms that inner spans are non-contiguous (they contain the `> ` prefix interleaved with content).
- `BulletList`: Outer span = `* ...\n* ...\n\n` (includes trailing blank line!).
  - Inner item spans (Plain blocks): content only, e.g., `"First item\n"` at `[11, 22)` — does NOT include the `* ` prefix.
  - Multi-line items: `"First item\n  continued here.\n"` — includes the continuation indent.
- `OrderedList`: Same pattern as BulletList. Inner items: `"First\n"` at `[3, 9)` — no `1. ` prefix.
- **Note**: BulletList span includes a trailing blank line in the span (`\n\n` at end), so there's a **zero-width gap** to the next block. This is different from most other blocks.

**Container blocks — NOT indentation boundaries:**
- `Div` (fenced): Span = `::: {.note}\n\nInner paragraph.\n\n:::\n`. Inner blocks have spans pointing into the div content area (e.g., Para at `[22, 39)`).
- Div inner block spans do NOT include `:::` fences — clean contiguous content.

**Desugared constructs:**
- `Table` (from list-table): Source span covers original `::: {.list-table}\n...\n:::\n` text. Confirms Option A works for `KeepBefore`.
- `DefinitionList` (from definition-list div): Same — span covers original `:::{.definition-list}\n...\n:::\n`.

**Metadata:**
- YAML front matter: Parsed into `doc.meta`, not as a block. First block starts after the `---\n...\n---\n` region.
- Inner metadata (`---\nkey: value\n---`): The metadata block itself is NOT in `doc.blocks` (absorbed into meta). The surrounding paragraphs have their normal spans.

**Shortcodes:**
- Inline shortcode: `{{< video ... >}}` appears as a `Shortcode` inline within a Paragraph. Span = `[7, 40)` = `"{{< video https://example.com >}}"`.
- Standalone shortcode: Appears as a Paragraph containing a Shortcode inline. The Paragraph span covers the entire line.

**Nested structures:**
- Nested block quotes: Inner BlockQuote span includes the outer `> ` prefix (e.g., `"> Inner.\n> > Continued.\n"`). Deeply nested inner Para span includes ALL prefixes from continuation lines.
- Nested lists: Inner BulletList has its own span. Inner item Paragraphs include continuation indentation but not bullet markers.

### Implications for the Incremental Writer

1. **Top-level blocks are cleanly separable.** Gaps are consistently 1-byte `\n` separators. Assembly is straightforward: copy spans, insert `\n` between them.

2. **Indentation boundary classification is confirmed.** BlockQuote inner spans include `> ` prefixes from other lines, making them non-contiguous with respect to the content. BulletList/OrderedList inner items exclude bullet/number prefixes but include continuation indentation.

3. **BulletList trailing blank line** is a quirk: the list span absorbs the trailing `\n\n`, creating a zero-width gap to the next block. The assembly strategy must account for this (don't insert an extra separator after a list).

4. **Metadata blocks are invisible** in `doc.blocks` — they're stored in `doc.meta` as `ConfigValue`. The reconciler already handles this: `apply_reconciliation` uses `meta: executed.meta` (executed metadata wins entirely, no fine-grained reconciliation). For the incremental writer: preserve `original_qmd[0..first_block_start]` verbatim when metadata is unchanged; rewrite the front matter region when `original_ast.meta != new_ast.meta`.

## API Design (Finalized)

### Decision: Both functions, `incremental_write` first

We provide two functions:
1. `incremental_write` → `String` — the primary function, builds the output string directly
2. `compute_incremental_edits` → `Vec<TextEdit>` — derived, for the Automerge sync layer

`incremental_write` is implemented first (Phase 2). `compute_incremental_edits` is added in Phase 2 as well, but can be deferred if needed — the Automerge layer can use `incremental_write` and diff the result as an interim solution.

### Core Function Signatures

```rust
/// Incrementally write an AST, producing a new QMD string that preserves
/// unchanged portions of the original text.
///
/// # Arguments
/// * `original_qmd` - The original QMD source text
/// * `original_ast` - The AST produced by reading `original_qmd`
/// * `new_ast` - The modified AST (what the user wants written)
/// * `plan` - A reconciliation plan describing alignment between original_ast and new_ast
///
/// # Returns
/// A new QMD string where:
/// - Unchanged blocks are preserved verbatim from `original_qmd`
/// - Changed blocks are rewritten using the standard writer
/// - The result round-trips: `read(result) ≡ new_ast` (structural equality)
pub fn incremental_write(
    original_qmd: &str,
    original_ast: &Pandoc,
    new_ast: &Pandoc,
    plan: &ReconciliationPlan,
) -> Result<String, Vec<DiagnosticMessage>>

/// Compute minimal text edits to transform `original_qmd` into the incremental write result.
///
/// Each TextEdit describes a byte range in `original_qmd` to replace and the replacement text.
/// Edits are sorted by range.start and non-overlapping.
pub fn compute_incremental_edits(
    original_qmd: &str,
    original_ast: &Pandoc,
    new_ast: &Pandoc,
    plan: &ReconciliationPlan,
) -> Result<Vec<TextEdit>, Vec<DiagnosticMessage>>

pub struct TextEdit {
    /// Byte range in the original string to replace
    pub range: Range<usize>,
    /// Replacement text
    pub replacement: String,
}
```

### Source Span Requirements (Resolved)

The incremental writer depends on `SourceInfo::Original { start_offset, end_offset }` in each block for verbatim copying. The investigation tests (Phase 0) confirmed:
- Top-level block spans cover `content + trailing \n` — sufficient for verbatim copy
- Gaps between blocks are consistently 1-byte `\n` separators
- BulletList spans absorb trailing blank line (zero-width gap to next block)
- Metadata is in `doc.meta`, not `doc.blocks` — prefix region needs special handling

## Algorithm Design (Detailed)

### Step 1: Coarsen the Reconciliation Plan

The coarsening step converts the hierarchical `ReconciliationPlan` into a flat `Vec<CoarsenedEntry>`, one entry per result block:

```rust
enum CoarsenedEntry<'a> {
    /// Copy this byte range verbatim from original_qmd.
    /// The text includes the block content + trailing \n.
    Verbatim {
        byte_range: Range<usize>,
    },
    /// Rewrite this block using the standard writer.
    Rewrite {
        block: &'a Block,
    },
}
```

**Coarsening rules for Phase 2 (conservative):**

For each entry in `plan.block_alignments`:

| Alignment | Coarsened Entry | Rationale |
|---|---|---|
| `KeepBefore(idx)` | `Verbatim(span_of(original_ast.blocks[idx]))` | Block is unchanged — copy verbatim |
| `UseAfter(idx)` | `Rewrite(new_ast.blocks[idx])` | Block is new or completely changed |
| `RecurseIntoContainer { before_idx, after_idx }` | `Rewrite(new_ast.blocks[after_idx])` | Container children changed — conservative: rewrite entire block |

The conservative strategy rewrites any block that has ANY change, regardless of container type. This is correct for all block types (indentation boundaries, non-boundaries, and leaf blocks). The distinction between container types only matters for later optimization.

**Future optimization (post-Phase 3):** For `RecurseIntoContainer` on non-boundary containers (Div, Figure, NoteDefinitionFencedBlock), we could recursively coarsen the inner blocks, producing mixed Verbatim/Rewrite output within the container. This would require handling `:::` fences and inner separators. Not needed for Phase 2.

### Step 2: Assemble the Result String

Walk the coarsened entries in order and build the output string:

```
result = metadata_prefix + block_0 + separator_0 + block_1 + separator_1 + ... + block_n
```

**2a. Metadata prefix:**
- If `original_ast.meta ≡ new_ast.meta` (structural equality): copy `original_qmd[0..first_block_start]` verbatim
- If metadata changed: rewrite the front matter using `write_config_value_meta()`
- If no blocks exist: the entire document is metadata (or empty)

**2b. Block text:**
- `Verbatim { byte_range }`: copy `original_qmd[byte_range]`
- `Rewrite { block }`: call `write_block(block, &mut buf, &mut ctx)` — requires making `write_block` public (or adding a public wrapper)

**2c. Separator between blocks:**

Each block's text (whether verbatim or rewritten) ends with `\n`. Between blocks, we need a blank line, which means one additional `\n`. However, BulletList spans absorb a trailing blank line (text ends with `\n\n`), so no additional separator is needed after them.

The separator logic:

```rust
fn needs_separator_after(block_text: &str) -> bool {
    // Block text already ends with \n\n (e.g., BulletList) → no separator needed
    !block_text.ends_with("\n\n")
}
```

For transitions between two Verbatim blocks from consecutive original positions, we can optionally use the original gap verbatim for maximum fidelity:

```rust
if prev is Verbatim(orig_idx_i) && curr is Verbatim(orig_idx_j) && orig_idx_j == orig_idx_i + 1 {
    // Copy original gap verbatim (preserves exact whitespace)
    let gap_start = span_of(original_ast.blocks[orig_idx_i]).end;
    let gap_end = span_of(original_ast.blocks[orig_idx_j]).start;
    result.push_str(&original_qmd[gap_start..gap_end]);
} else if needs_separator_after(prev_block_text) {
    result.push('\n');
}
```

This preserves original gaps exactly for unchanged consecutive blocks (ensuring Property 2: idempotence) while using the standard `\n` separator when blocks are added, removed, or reordered.

### Step 3: compute_incremental_edits (derived from incremental_write)

`compute_incremental_edits` can be implemented by reasoning about the relationship between original and result block sequences:

1. Walk `block_alignments` and identify **runs of consecutive Verbatim blocks from consecutive original positions**. These runs correspond to unchanged regions in the original text.
2. The gaps between runs (where blocks were added, removed, or rewritten) become `TextEdit` entries.
3. Each `TextEdit` specifies: the byte range in the original to replace, and the replacement text (from the writer output).

Example: Original has blocks at spans [0,10), [11,20), [21,30). If block 1 is rewritten:
- Run 1: Verbatim blocks 0 → no edit for [0,10)
- Edit: replace [10,20) (gap + block 1) with `\n` + writer output
- Run 2: Verbatim block 2 → no edit for [20,30)
- Result: `[TextEdit { range: 10..20, replacement: "\nnew block 1 text\n" }]`

This produces minimal, non-overlapping edits sorted by position. The Automerge layer can apply these edits efficiently.

### Making write_block Accessible

The current `write_block()` in `writers/qmd.rs` is private. The incremental writer needs to call it for `Rewrite` blocks. Options:

1. **Make `write_block` public** — simplest, but exposes internal writer API
2. **Add a public wrapper** like `pub fn write_single_block(block: &Block) -> Result<String>` — cleaner API boundary
3. **Use the existing `write()` function with a single-block Pandoc** — wasteful (creates metadata, etc.)

**Decision**: Option 2 — add a public `write_single_block` function that wraps `write_block` with a fresh `QmdWriterContext`. This keeps the internal writer details private while providing exactly what the incremental writer needs.

## Properties for Property-Based Testing

### Property 1: Round-Trip Correctness

```
∀ original_qmd, ast_change:
  let original_ast = read(original_qmd)
  let new_ast = apply_change(original_ast, ast_change)
  let plan = reconcile(original_ast, new_ast)
  let result_qmd = incremental_write(original_qmd, original_ast, new_ast, plan)
  read(result_qmd) = new_ast
```

This is the fundamental correctness property.

### Property 2: Idempotence on Unchanged ASTs

```
∀ original_qmd:
  let ast = read(original_qmd)
  let plan = reconcile(ast, ast)        // identity reconciliation
  incremental_write(original_qmd, ast, ast, plan) = original_qmd
```

If nothing changed, the output should be byte-for-byte identical to the input.

### Property 3: Equivalence with Full Writer

```
∀ original_qmd, new_ast:
  let original_ast = read(original_qmd)
  let plan = reconcile(original_ast, new_ast)
  let incremental_result = incremental_write(original_qmd, original_ast, new_ast, plan)
  read(incremental_result) = read(write(new_ast))
```

The incremental writer and the full writer should produce semantically equivalent documents (though not necessarily byte-identical).

### Property 4: Locality (the key incrementality property)

```
∀ original_qmd, single_block_change at index i:
  let original_ast = read(original_qmd)
  let new_ast = apply_single_block_change(original_ast, i, change)
  let plan = reconcile(original_ast, new_ast)
  let edits = compute_incremental_edits(original_qmd, original_ast, new_ast, plan)

  // Edits should not touch blocks far from index i
  // Specifically: for all blocks j where j != i and block j is not
  // an indentation ancestor of block i:
  //   original_qmd[span(block_j)] is not overlapped by any edit
```

This is the property that distinguishes the incremental writer from the trivial `write(new_ast)`.

### Property 5: Monotonicity of Edit Spans

```
∀ edits from compute_incremental_edits:
  edits are sorted by range.start
  no two edits overlap
```

### Testing Strategy: Generators (Implementation Assessment)

#### Existing Infrastructure (quarto-ast-reconcile/src/generators.rs)

The generators produce ASTs at all complexity levels, with configurable feature flags:
- `GenConfig::minimal()` — paragraphs with Str/Space only
- `GenConfig::full()` — all block and inline types
- `GenConfig::with_lists()` — lists with varying item counts
- `gen_pandoc()`, `gen_blocks()`, `gen_inlines()` — configurable generators

**Key limitation:** Generated ASTs have `dummy_source()` (all offsets = 0). They cannot be used directly as `original_ast` for the incremental writer, which needs accurate source spans.

#### The Write-Read Pipeline (Key Enabler)

To get ASTs with accurate source spans, we use the **write-read pipeline**:

```rust
// Generate a raw AST (no source spans)
let raw_ast = gen_pandoc(config);
// Write to QMD text
let original_qmd = write(raw_ast);
// Read back — now has accurate source spans into original_qmd
let original_ast = read(&original_qmd);
```

This produces a canonical `(original_qmd, original_ast)` pair where source spans are guaranteed accurate. The `original_ast` may differ from `raw_ast` due to reader postprocessing (merge_strs, auto-IDs, etc.), but that's fine — we care about the canonical form.

**Concern: Not all generated ASTs round-trip through write→read.** Some generated features may not be supported by the writer, or the reader may interpret the writer's output differently. We must use **restricted generator configs** that produce round-trippable ASTs, starting with the simplest and expanding:

1. **Level 0:** `GenConfig::minimal()` — paragraphs with Str/Space. Safest.
2. **Level 1:** Add headers, code blocks, horizontal rules. Leaf blocks only.
3. **Level 2:** Add block quotes, bullet lists, ordered lists. Container blocks.
4. **Level 3:** Add divs, definition lists, tables. Complex blocks.

Each level should be verified to round-trip before use in property tests.

#### New Generators Needed: AST Mutations

The incremental writer tests need **AST mutation generators** that produce `(original_ast, new_ast)` pairs. These are NEW and specific to the incremental writer:

```rust
enum AstMutation {
    /// Change the text content of a random Str inline
    ChangeStrText { block_idx: usize, new_text: String },
    /// Add a new paragraph at a given position
    InsertBlock { position: usize, block: Block },
    /// Remove a block at a given position
    RemoveBlock { position: usize },
    /// Add a list item to a list
    AddListItem { block_idx: usize, item: Vec<Block> },
    /// Remove a list item from a list
    RemoveListItem { block_idx: usize, item_idx: usize },
    /// Change emphasis on inlines (wrap/unwrap in Emph)
    ToggleEmphasis { block_idx: usize },
}
```

The mutation generator works as:
1. Generate `original_ast` via write-read pipeline
2. Generate a random `AstMutation` valid for this AST structure
3. Apply mutation to produce `new_ast`
4. Return `(original_qmd, original_ast, new_ast)`

#### Phasing of Property Tests

| Property | Requires | Phase |
|---|---|---|
| Property 2 (Idempotence) | Only write-read pipeline, no mutations | Phase 1 |
| Property 1 (Round-trip correctness) | Write-read pipeline + mutation generators | Phase 2 |
| Property 3 (Equivalence with full writer) | Same as Property 1 | Phase 3 |
| Property 4 (Locality) | `compute_incremental_edits` + mutations | Phase 3 |
| Property 5 (Monotonicity) | `compute_incremental_edits` | Phase 3 |

Property 2 is the simplest to test and should be the FIRST property verified. It doesn't need mutation generators — just verify that `incremental_write(qmd, ast, ast, identity_plan) == qmd`.

## Work Items

### Phase 0: Design Iteration (current)
- [x] Write initial design document
- [x] Analyze sugar/desugar pipeline interaction
- [x] Investigate source span coverage for top-level blocks
  - Spans are "content + trailing `\n`", gaps are single `\n` separators
  - BulletList absorbs trailing blank line (zero-width gap to next block)
  - Metadata blocks not in `doc.blocks` (front matter region needs special handling)
- [x] Prototype source span verification (20 tests in `tests/incremental_writer_investigation.rs`)
- [x] Run concrete experiments on block separator handling — see "Source Span Experimental Findings" section
- [x] Finalize the "indentation boundary" classification (complete: 3 boundaries, 3 non-boundary containers, 2 always-rewrite, 11 leaf blocks)
- [x] Catalog all sugar/desugar transforms and verify none break the safe-rewrite-boundary assumption (only 2 pairs: list-table, definition-list — both safe)
- [x] Add source comments at sugar/desugar sites noting incremental writer coupling (postprocess.rs:transform_divs, qmd.rs:write_table, write_list_table, write_definitionlist)
- [x] Add central transform registry doc comment in `postprocess.rs` (registry table at top of file)
- [x] Decide on API: both functions — `incremental_write` primary, `compute_incremental_edits` derived (see "API Design" section)
- [x] Design the coarsening algorithm in detail (see "Algorithm Design" section — 3-step: coarsen, assemble, compute edits)
- [x] Review and iterate on properties with property-based testing in mind (see "Testing Strategy: Generators" — write-read pipeline, restricted configs, mutation generators, phased testing)

### Phase 1: Infrastructure and Testing Framework
- [x] Create `writers::incremental` module within pampa
- [x] Add `write_single_block` and `write_metadata` public wrappers to `qmd.rs`
- [x] Set up proptest infrastructure
  - [x] QMD string generators (4 levels: paragraphs → leaf blocks → containers → front matter)
  - [x] QMD mutation generators (single mutation, add block, remove block, mixed types)
  - [x] Property 1 proptest (round-trip correctness) — 4 generators, all passing
  - [x] Property 2 proptest (idempotence) — 4 levels, all passing
- [x] Implement source span investigation as unit tests (20 tests in `tests/incremental_writer_investigation.rs`)
- [x] Add sugar/desugar roundtrip tests: Table roundtrips correctly; DefinitionList is LOSSY (writer produces Pandoc-native syntax, reader expects div syntax — ignored tests document this pre-existing bug)

### Phase 2: Core Implementation
- [x] Implement plan coarsening (conservative: rewrite entire changed blocks)
- [x] Implement result assembly from coarsened plan
- [x] Implement `incremental_write()`
- [x] Implement `compute_incremental_edits()` (simple: single edit for whole doc, future: minimal edits)
- [x] Hand-crafted idempotence tests passing (17 tests)
- [x] Hand-crafted round-trip tests passing (8 tests + 1 verbatim preservation)
- [x] Fix metadata prefix bug (extra `\n` before first block with YAML front matter)
- [x] Fix removed-first-block bug (falsely triggering metadata prefix)
- [x] Make Property 1 and Property 2 proptests pass (36 tests total: 28 hand-crafted + 8 proptests)

### Phase 3: Property Verification
- [x] Implement Property 3 proptest (equivalence with full writer)
- [x] Implement Property 4 proptest (verbatim preservation of unchanged blocks — weaker form; strong form requires fine-grained edits)
- [x] Implement Property 5 proptest (monotonicity of edit spans + identity produces zero edits)
- [x] All property tests green (40 tests total: 28 hand-crafted + 12 proptests)

### Phase 4: Integration
- [x] Wire into WASM module (`wasm-quarto-hub-client`) — `incremental_write_qmd(original_qmd, new_ast_json)` export added
- [x] Wire into `quarto-sync-client` `updateFileAst` path — `ASTOptions.incrementalWriteQmd` optional, fallback to `writeQmd`
- [x] Wire into hub-react-todo demo — `wasm.ts` wrapper + `useSyncedAst.ts` passes `incrementalWriteQmd`
- [ ] End-to-end: hub-react-todo checkbox toggle writes back to document (see Phase 4b below)

### Phase 4b: End-to-End — Checkbox Toggle in hub-react-todo

Minimal end-to-end demo: when a user clicks a checkbox in the todo list UI, the change flows through the incremental writer and back into the synced QMD document.

#### Current state of the demo

The demo app has these components:
- **`App.tsx`** — Root, hardcodes sync server + doc ID + file path
- **`TodoApp.tsx`** — Connects `useSyncedAst` hook to `TodoList` component via `astHelpers`
- **`TodoList.tsx`** — Renders checkboxes. Already has an `onToggle?: (index: number) => void` prop, but it's never passed (checkboxes are disabled/read-only)
- **`useSyncedAst.ts`** — React hook that creates a sync client with AST options, returns `{ ast, connected, error, connecting }`. Stores the client in a ref but does **not** expose `updateFileAst`
- **`astHelpers.ts`** — `findTodoDiv(ast)` finds the `Div#todo` block, `extractTodoItems(todoDiv)` extracts `{ checked, label, itemIndex }` from the BulletList inside it

#### AST structure for checkboxes

QMD source:
```
:::{#todo}
- [ ] Unchecked item
- [x] Checked item
:::
```

Parsed AST (simplified):
```
Div (id="todo")
  BulletList
    Item 0: [Plain: [Span([], []),        Space, Str("Unchecked"), Space, Str("item")]]   ← unchecked
    Item 1: [Plain: [Span([], [Str("x")]), Space, Str("Checked"),  Space, Str("item")]]   ← checked
```

The checkbox state lives in the `Span`'s inline content:
- **Unchecked**: `Span(["", [], []], [])` — empty inline content
- **Checked**: `Span(["", [], []], [Str("x")])` — contains `Str("x")`

To toggle: mutate the Span's `c[1]` (inline content array) between `[]` and `[{t: "Str", c: "x", s: 0}]`.

#### Implementation steps

- [x] **Step 1: Add `toggleCheckbox` to `astHelpers.ts`**
  - Takes `(ast: RustQmdJson, itemIndex: number) → RustQmdJson | null`
  - Deep-clones via `JSON.parse(JSON.stringify(ast))`
  - Navigates to `Div#todo → BulletList → items[itemIndex] → Plain → inlines[0] (Span)`
  - Toggles: if Span content has Str("x"), clears to `[]`; otherwise sets to `[{t: "Str", c: "x", s: 0}]`
  - Returns null on navigation failure

- [x] **Step 2: Expose `updateFileAst` from `useSyncedAst`**
  - Added `updateAst: ((ast: RustQmdJson) => void) | null` to `SyncedAstState`
  - Stable `useCallback` that calls `clientRef.current.updateFileAst(filePathRef.current, ast)`
  - Returns `updateAst` when connected, null otherwise

- [x] **Step 3: Wire `onToggle` through `TodoApp` → `TodoList`**
  - `TodoApp` destructures `updateAst` from hook, passes to `TodoFromAst`
  - `TodoFromAst` creates `onToggle` callback: `toggleCheckbox(ast, itemIndex)` → `updateAst(newAst)`
  - `TodoList` receives `onToggle` (enables checkboxes) or undefined (disables them)

- [ ] **Step 4: Verify the round-trip**
  - `npm run build:all` succeeds
  - Run the demo against a live sync server (or local test)
  - Toggle a checkbox → the QMD text in the synced document changes (only the checkbox span, rest preserved verbatim)
  - The AST change notification fires back → UI updates to reflect the new state

#### Data flow

```
User clicks checkbox
  → TodoList.onToggle(itemIndex)
  → TodoApp: newAst = toggleCheckbox(ast, itemIndex)
  → useSyncedAst: client.updateFileAst(filePath, newAst)
  → quarto-sync-client: incrementalWriteQmd(cachedSource, newAst)
  → WASM: parse original → reconcile → incremental_write → result QMD
  → sync client: updateText(doc, ['text'], resultQmd)  [Automerge diffs]
  → Automerge syncs to peers
  → onFileChanged fires → tryParseAndNotify → onASTChanged
  → useSyncedAst: setAst(newAst) → React re-renders with updated checkbox
```

### Phase 5 (Future): Inline Splicing
- [ ] Investigate inline source span contiguity
- [ ] Implement inline-level splicing for Paragraph/Plain/Header blocks
- [ ] Handle the SoftBreak/LineBreak indentation interaction (see design note below)
- [ ] Property tests for inline-level incrementality

#### Design Note: SoftBreak and LineBreak Have Indentation Consequences

Inline splicing is **not safe** when changes involve `SoftBreak` or `LineBreak` inside an indentation boundary. Example:

**Old QMD:**
```
> Hello world
```
**Old AST:** `BlockQuote [Para [Str "Hello world"]]`

**New AST:** `BlockQuote [Para [Str "Hello", SoftBreak, Str "world"]]`

**Correct new QMD:**
```
> Hello
> world
```

The change happened only in the inner paragraph's inlines, but the new `SoftBreak` creates a new line that requires the `> ` prefix from the enclosing `BlockQuote`. Naively splicing the inline content would produce:

```
> Hello
world
```

which is incorrect (the second line would exit the block quote).

**Rule for Phase 5:** Inline splicing inside an indentation boundary is only safe if the change does not add or remove `SoftBreak` or `LineBreak` inlines. If a SoftBreak/LineBreak is added, removed, or moved, the entire indentation boundary must be rewritten.

At top level (no enclosing indentation boundary), SoftBreak/LineBreak changes are safe to splice because there's no prefix to propagate.

## Open Questions

1–2. ~~**Source span coverage and block separator handling**~~: **Resolved by experiments.** Block spans are "content + trailing `\n`" with 1-byte `\n` gaps between them. Assembly strategy: copy spans verbatim, insert `\n` separator between blocks. One quirk: BulletList absorbs the trailing blank line into its span (zero-width gap to next block). YAML front matter is not in `doc.blocks` — the incremental writer must preserve the prefix before the first block's start offset. See "Source Span Experimental Findings" section for full details.

3. ~~**Metadata (YAML front matter)**~~: **Resolved.** Start with "no" — the incremental writer does not handle metadata incrementally. Any metadata change in the reconciliation plan rewrites the entire metadata node (front matter or inner metadata block). May revisit in the future.

4. ~~**Reconciliation plan reuse**~~: **Resolved — no blocking assumptions.** The reconciler is completely use-case agnostic. The three-phase algorithm (hash match, positional type match, fallback) makes no assumptions about the relationship between "before" and "after". The existing property-based tests already validate with arbitrary, unrelated AST pairs. Source info handling is neutral (before's source for kept nodes, after's source for replaced nodes). Safe to reuse for `(original, user-edited)` pairs with no modifications.

5. ~~**Crate placement**~~: **Resolved.** The incremental writer will be a **module within pampa** (e.g., `writers::incremental`). Rationale: pampa already depends on `quarto-ast-reconcile`, so it has access to both the reconciliation plan types and the QMD writer. Putting it in `quarto-ast-reconcile` would create a circular dependency (it would need pampa's writer). A new crate is unnecessary overhead.

6. ~~**The exact definition of structural equality for the round-trip property**~~: **Resolved.** Structural equality (ignoring source info) is the correct definition. Source info _must_ be excluded because `Verbatim` copying preserves text but shifts byte offsets of all subsequent blocks — so `read(result_qmd)` will always have different source offsets than `new_ast` for blocks after any `Rewrite` region. Source info is the primary field where strict equality is impossible; other fields are either content-derived (deterministic, e.g., auto-generated header IDs) or semantic (not position-dependent). **Note:** The reader's `merge_strs` postprocessing could also cause structural differences (merging adjacent `Str` nodes), but this is an existing concern shared with the non-incremental round-trip property and not specific to the incremental writer.

7. ~~**Sugar/desugar pipeline stage for reconciliation**~~: **Resolved — Option A, with defensive measures.** We reconcile post-desugared ASTs. To minimize future risk, we apply these defensive practices:

   **a. Source comments at sugar/desugar sites.** Add warnings at these locations noting the coupling with the incremental writer:
   - `postprocess.rs:761` (`transform_divs`) — the desugar entry point
   - `qmd.rs:1053` (`write_table`) — the sugar decision point (pipe vs list-table)
   - `qmd.rs:866` (`write_list_table`) — list-table sugaring
   - `qmd.rs:667` (`write_definitionlist`) — definition-list sugaring

   **b. Central transform registry.** Add a module-level doc comment in `postprocess.rs` (or a dedicated section) enumerating all sugar/desugar transforms with their properties:
   - Transform name
   - Read-path: what it converts from/to
   - Write-path: what it converts from/to
   - Whether the sugared form uses indentation boundaries (affects incremental writer safety)

   Anyone adding a new transform must update this registry and consider incremental writer implications.

   **c. Sugar/desugar roundtrip property test.** Verify `desugar(sugar(node)) ≡ node` for all affected node types (Table, DefinitionList). If a future sugar transform is lossy, this catches it before it silently breaks the incremental writer. This is a property that the writer should already satisfy, but an explicit test dedicated to this concern provides defense in depth.

   **d. Incremental writer safety check for new transforms.** When adding a new sugar/desugar transform, document whether the sugared representation uses indentation boundaries. If it does, the safe-rewrite-boundary rule handles it automatically (full rewrite). If it does NOT use indentation boundaries, Option A may be insufficient and we'd need to evaluate whether reconciliation at the post-desugared level can correctly splice the sugared form.

8. ~~**Write-path sugar non-determinism**~~: **Resolved.** Format changes on `Rewrite` are acceptable. This is a canonicalization artifact — analogous to the writer choosing `*` vs `_` for emphasis. The round-trip property (semantic correctness) is the non-negotiable invariant; formatting preservation is best-effort and only guaranteed for `KeepBefore`/`Verbatim` blocks.

## References

- `crates/pampa/src/writers/qmd.rs` — current QMD writer (includes Table→list-table sugaring)
- `crates/pampa/src/readers/qmd.rs` — current QMD reader
- `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs` — desugaring transforms (list-table Div→Table, definition-list Div→DefinitionList)
- `crates/quarto-ast-reconcile/` — reconciliation algorithm and types
- `crates/quarto-ast-reconcile/src/generators.rs` — proptest generators for ASTs
- `crates/quarto-pandoc-types/` — AST type definitions
- `crates/quarto-source-map/src/source_info.rs` — source span types
