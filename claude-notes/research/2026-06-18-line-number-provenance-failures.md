# Line-number provenance failures — empirical examples

**Date:** 2026-06-18
**Branch:** feature/block-editing-improvements (worktree `.worktrees/block-editing`)
**Status:** Research / reproduced. Both problems demonstrated with real pampa
functions on small fixtures (3 examples each, distinct Pandoc elements).

> **How these were produced.** A throwaway probe
> (`crates/pampa/examples/provenance_probe.rs`, since removed) drove the *real*
> functions — `regenerate_nested_buffers_ast`, `write_with_source_info`,
> `SourceInfo::map_offset` — on each fixture and printed the computed vs. actual
> line numbers. Every number below is observed output, not hand-derived.

---

## Summary

Two different features rely on a "what source line is this?" mapping, and both
get it **wrong** for the same underlying reason: **the qmd writer does not
preserve source line *count*** (it collapses blank lines — a loose list becomes
tight, a double blank becomes single), but both consumers assume a **linear**
output↔source correspondence.

- **Problem 1 — nest-in line number** (block-editing nesting cursor). The editor
  maps a caret in the re-serialized "clean buffer" back to a source line by
  `Ls = lineOf(blockStart) + bufferLine`. When the buffer has fewer lines than
  the source span, `Ls` is **too small** — by 1–2 lines in the examples — so
  nest-in / arrow navigation resolves the **wrong surface**.
- **Problem 2 — engine error line provenance** (`write_with_source_info`, used to
  map execution-engine errors back to source). `map_offset` shifts an output byte
  linearly within a **single per-top-level-block piece**. A code cell **nested**
  in a container inherits that one linear piece; when the writer collapses blank
  lines before the code, the engine error maps to the **wrong source line** (the
  fence line above the real code), **silently**.

---

## Problem 1 — nest-in line number

### Patient explanation

When you click into a block in the live preview, you are not editing the raw
source. For a **nested, multi-line** block (a block under a `>`/list/def-list
prefix), the editor seeds the textarea from a **clean buffer** —
`regenerate_nested_buffers` re-serializes just that block with the ancestor
prefix stripped (`crates/pampa/src/regenerate_nested_buffers.rs`, via
`write_single_block`).

The nesting cursor then maps a caret position in that buffer back to a **source
line** so it can decide where to nest-in / step to:

```
Ls = map.lineOf(et.anchorR0) + live.bufferLine
   (ts-packages/preview-renderer/src/q2-preview/PreviewRoot.tsx:1258)
```

i.e. *buffer line N is assumed to be source line `lineOf(blockStart) + N`*. That
assumption holds **only if the clean buffer has the same number of lines, in the
same order, as the block's source span.**

It doesn't. The qmd writer **normalizes blank lines**: a **loose** list (items
separated by blank lines) is re-serialized **tight** (no blanks), and a run of
blank lines inside a blockquote collapses to one. So the buffer has **fewer**
lines than the source span, and every buffer line *after* the collapse maps to a
source line that is too small. The caret lands on the wrong source line, and
`surfaceAtLine` / `childSurfaceTowardLine` resolve the wrong block — the observed
"nest-in goes to the parent."

### Example N1 — `BlockQuote` (nested), two paragraphs

Source (the surface being edited is the **inner** blockquote):

```text
> > A
> >
> >
> > B
```

The inner blockquote's source span is 4 lines (`A`, blank, blank, `B`). The clean
buffer the editor shows collapses the double blank to one:

```text
> A
> 
> B
```

**Bad line numbers** (observed): a caret on buffer line 2 (`> B`) →
`Ls = lineOf(blockStart=0) + 2 = 2`. But `B` is on **source line 3**.
**Off by −1.** Nest/nav from `B` resolves as if it were a line earlier.

### Example N2 — `BulletList` (sublist), loose → tight

Source (the surface is the **sublist** `b`/`c` under `outer`):

```text
- outer
  - b


  - c
```

The sublist's source span is 4 lines (`b`, blank, blank, `c`). The clean buffer
re-serializes it **tight**:

```text
* b
* c
```

**Bad line numbers** (observed): a caret on buffer line 1 (`* c`) →
`Ls = lineOf(blockStart=1) + 1 = 2`. But `c` is on **source line 4**.
**Off by −2.**

### Example N3 — `OrderedList` (sublist), loose → tight

Source:

```text
- outer
  1. b


  2. c
```

Buffer (tight):

```text
1.  b
2.  c
```

**Bad line numbers** (observed): caret on buffer line 1 (`2.  c`) →
`Ls = 1 + 1 = 2`. But `c` is on **source line 4**. **Off by −2.**

### Why it matters

`Ls` is the input to surface resolution. An `Ls` that is 1–2 lines too small
means the nesting cursor believes the caret is inside an earlier/shallower
surface than it is, so nest-in descends from the wrong place or lands on the
parent. The error is **exactly the line delta the writer introduced** by
dropping blank lines, and it grows with the number of collapsed blanks.

---

## Problem 2 — engine error line provenance

### Patient explanation

When the document is handed to an execution engine, pampa serializes the AST and
keeps a provenance object so an engine error at "line N of what we gave you" can
be mapped back to the original source:

```
let (qmd, source_info) = write_with_source_info(ast);   // crates/pampa/src/writers/qmd.rs:2544
... source_info.map_offset(byte_in_qmd, &source_context) // -> original file/row/col
```

The provenance is built by `write_impl_tracked` (qmd.rs:2571), which tiles the
output with **one piece per *top-level* block**: `(block.source_info(), len)`. It
**does not recurse into containers.** And `map_offset` on a leaf piece is a
**pure linear byte shift** — `source_offset = piece_source_start + output_offset`
(`crates/quarto-source-map/src/mapping.rs:29`) — exact **only** when the output
bytes are byte-identical to source.

That breaks for a **code cell nested inside a container** (`Div`, `BlockQuote`,
list). The container is the top-level block, so the *whole container* is **one
linear piece** and the nested code inherits it (confirmed below: the dumped
`source_info` is `Concat (1 piece)`). When the writer collapses blank lines
*before* the code inside that container, the code's **output** byte offset is
smaller than its **source** offset, so the linear map lands **earlier** in source
— on the fence line above the actual code. There is **no warning**.

### Example E1 — `Div` containing prose + collapsing blanks + code

Source:

```text
::: {.note}
intro




```
BOOM
```
:::
```

`BOOM` is on **source line 8**. The serialized output the engine sees (blank run
collapsed):

```text
::: {.note}

intro

```
BOOM
```

:::
```

Provenance dumped: `Concat (1 piece) [out 0..38] -> Original src[0..40]` — the
**entire Div is one linear piece**.

**Bad line number** (observed): `map_offset(byteof("BOOM"))` → **source line 7**;
actual is **source line 8**. **Off by −1** — it points at the opening ` ``` `
fence, not the code.

### Example E2 — `BlockQuote` containing prose + collapsing blanks + code

Source:

```text
> intro
>
>
>
>
> ```
> BOOM
> ```
```

`BOOM` is on **source line 7**. Output (blank quote-lines collapsed to one):

```text
> intro
> 
> ```
> BOOM
> ```
```

Provenance: `Concat (1 piece) [out 0..30] -> Original src[0..37]`.

**Bad line number** (observed): `map_offset` → **source line 6**; actual **7**.
**Off by −1** — points at `> ``` ` instead of `> BOOM`.

### Example E3 — `Div` containing a `BulletList` + collapsing blanks + code

Source:

```text
::: {.note}
- a
- b




```
BOOM
```
:::
```

`BOOM` is on **source line 8**. Output:

```text
::: {.note}

* a
* b

```
BOOM
```

:::
```

Provenance: `Concat (1 piece) [out 0..40] -> Original src[0..41]`.

**Bad line number** (observed): `map_offset` → **source line 7**; actual **8**.
**Off by −1.**

### Why it matters, and the difference from Problem 1

For a **top-level** code cell the per-block re-anchoring saves it (its own piece,
verbatim body) — that's why everyday engine error mapping mostly works. The
failure is specifically **container-nested** code, where the single linear piece
spans the collapsed blanks. The magnitude here is −1 line (a single collapsed
blank-run); it scales with how much the writer reshapes the container before the
code. Unlike the nesting cursor's column path (which `warn`s when its assumption
breaks), this mis-mapping is **completely silent**.

---

## Common root cause

Both are the same defect viewed twice:

> The qmd writer does **not** preserve source **line count** (it collapses blank
> lines / loosens-to-tightens), but both consumers assume a **linear**
> output→source line correspondence — `lineOf(blockStart) + bufferLine` for
> nest-in, and `piece_source_start + output_offset` for the engine.

The fix shape is the same for both: stop assuming the correspondence and have the
writer **emit** it. Two designs were scoped on 2026-06-18:

- **Per-line provenance** (`claude-notes/plans/2026-06-18-qmd-per-line-provenance.md`)
  — record one line-anchor per *leaf block*; container-inserted/dropped lines
  become "synthesized" and are skipped. Fixes Problem 1 directly, and is the same
  generalization (recurse the per-block tiling into containers) that would fix
  Problem 2's nested-cell case.
- **Per-leaf-inline provenance** — the heavier variant that additionally makes the
  *column* exact. Not needed for either line-number failure above.

## Reproduction note

The probe binary `crates/pampa/examples/provenance_probe.rs` was used to generate
every figure here and then removed (it is throwaway, not a test). To regenerate,
re-create a small example that calls `regenerate_nested_buffers_ast` (Problem 1)
and `write_with_source_info` + `SourceInfo::map_offset` (Problem 2) on these
fixtures and prints `Ls`/`map_offset` against the known source lines.
