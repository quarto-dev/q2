# qmd writer — leaf-block source provenance (fixes nest-in **and** engine line numbers)

**Date:** 2026-06-18
**Branch:** feature/block-editing-improvements (worktree `.worktrees/block-editing`)
**Status:** PLAN — design settled with the user across the 2026-06-18 provenance
investigation; both target failures reproduced empirically (see research note).
TDD-first; pampa-only.

> **SCOPE.** pampa qmd-writer methods and their pampa-native tests only. This now
> covers BOTH (a) a **new** single-block method for the nesting cursor, and (b) a
> **reimplementation of the existing `write_with_source_info`** to fix engine
> error-line mapping. It does **not** touch the frontend,
> `regenerate_nested_buffers.rs`, the `nestedEditBuffers`/engine payloads, or any
> consumer wiring — those are out of scope. Column-exact (per-leaf-inline)
> provenance is also out of scope (see §10).

> **Evidence base.** Both failures are reproduced with real functions in
> `claude-notes/research/2026-06-18-line-number-provenance-failures.md` (3
> examples each, distinct Pandoc elements). Those fixtures become the regression
> tests here.

---

## 1. Motivation — two failures, one root cause

Two features rely on an output↔source line mapping; both are wrong, for the same
reason.

**Problem 1 — nest-in line number** (block-editing). The nesting cursor maps a
caret in a re-serialized clean buffer to a source line via `Ls =
lineOf(blockStart) + bufferLine` (`PreviewRoot.tsx:1258`). When the writer
collapses blank lines (loose list → tight; double blank → single), the buffer has
fewer lines than the source span, so `Ls` is too small and nest-in resolves the
wrong surface. Observed: off by −1 to −2 (research §Problem 1, N1–N3).

**Problem 2 — engine error line provenance.** `write_with_source_info`
(qmd.rs:2544 → `write_impl_tracked` qmd.rs:2571) tiles the output with **one piece
per *top-level* block** and `map_offset` shifts **linearly** within a piece
(`mapping.rs:29`). A code cell **nested in a container** (Div/BlockQuote/list)
inherits that one linear piece (confirmed: dumped `source_info` is
`Concat (1 piece)`); collapsed blanks before the code make `map_offset` land on
the wrong source line — **silently**. Observed: off by −1, landing on the fence
line above the code (research §Problem 2, E1–E3).

**Shared root cause.** The qmd writer does not preserve source **line count**, but
both consumers assume a **linear** output→source correspondence. The cure for both
is to stop assuming it and have the writer **record** where each leaf block
actually landed — *recursed through containers*, so a nested block (code cell, list
item, quoted paragraph) is anchored at its **own** source start instead of
inheriting a distant ancestor's linear map.

---

## 2. Unified design

One tracking **core**; two **projections**.

### 2.1 Leaf blocks vs. container blocks

Every block routes through `write_block` (qmd.rs:2275):

- **Leaf blocks** emit their own text, recurse no further: `Plain`, `Paragraph`,
  `Header`, `CodeBlock`, `RawBlock`, `LineBlock`, `HorizontalRule` (+ note-def /
  metadata leaves — classify during impl, §5).
- **Container blocks** emit structure (markers, fences, prefixes, blank
  separators, indentation) and delegate content back to `write_block`:
  `BlockQuote`, `BulletList`, `OrderedList`, `DefinitionList`, `Div`, `Figure`.

All content is from leaf blocks; all line-structure rearrangement is from
containers.

### 2.2 Counting bottom sink (true output offset under the wrappers)

Container prefixes are injected by `Write` wrappers (`BlockQuoteContext`
qmd.rs:109, `BulletListContext` qmd.rs:145) passed *as* `buf` (qmd.rs:447, 519),
so `buf.len()` is the wrong coordinate space. Add a thin `Write` adapter wrapping
the real `Vec<u8>` that exposes the running output length via `Rc<Cell<usize>>` in
`ctx` (`ctx.out_pos()`). `Rc`/`Cell` is fine (qmd writing is single-threaded,
native and WASM).

### 2.3 Recursive leaf-block instrumentation → provenance records

In `write_block`, bracket **only** the leaf-block arms; recurse through containers
unchanged:

```rust
let start = ctx.out_pos();
write_paragraph(para, buf, ctx)?;                 // existing leaf writer
ctx.record_leaf(para.source_info(), start, ctx.out_pos());
```

This runs at **every depth** (a code block nested three containers deep is still a
leaf and still gets recorded). The result is a list of records
`(leaf_source_info, out_byte_start, out_byte_end)`. Everything *not* recorded — the
container glue: markers, fences, prefixes-on-their-own-lines, inserted/collapsed
blank lines — is **glue**.

**Why containers need no instrumentation.** Container output is one of two things:
1. **Bytes on a content line** (a `* ` marker, a `> ` prefix) — injected by the
   wrapper *during* a leaf's write, so they fall *inside* that leaf's
   `[start,end)`. Harmless: at line granularity the line maps to the leaf; at byte
   granularity in the whole-doc path the prefix exists verbatim in source too (see
   §2.6).
2. **A whole glue line** (a loose-list blank, a `:::` fence line, an empty-item
   marker line) — recorded by no leaf → it is **glue** → `Generated` /
   *synthesized*.

### 2.4 Projection A — `SourceInfo` (fixes the ENGINE path)

Assemble a `SourceInfo::concat` that tiles the whole output: each leaf record →
`(leaf.source_info, len)`; each glue gap → `Generated{by: writer}`. **Reimplement
`write_with_source_info` on this recursive core** (replacing the top-level-only
`write_impl_tracked`). Now a nested code cell has its **own** `Original` piece
anchored at the cell's source start, so `map_offset` of a code byte is exact
(code bodies are verbatim — §2.6) and the collapsed blanks before it are glue,
not part of the cell's anchor. **This makes E1–E3 map correctly.**

Signature unchanged: `write_with_source_info(&Pandoc) -> (Vec<u8>, SourceInfo)`.
Callers (engine path) are untouched; behavior is strictly finer. Backward-compat
handling: §3.

### 2.5 Projection B — `Vec<BlockLineSpan>` (fixes the NEST-IN path)

```rust
pub struct BlockLineSpan {
    pub out_line_start: u32,        // output line where this leaf begins
    pub out_line_count: u32,        // output lines it occupies (≥ 1)
    pub source_byte_start: usize,   // leaf.source_info().start_offset
}
pub fn write_block_with_line_spans(block: &Block)
    -> Result<(Vec<u8>, Vec<BlockLineSpan>), Vec<DiagnosticMessage>>;
```

Same core, restricted to one block; convert each leaf record's out-byte range to
out-line range (one `\n` scan of the buffer). Glue lines are covered by no span →
**synthesized** (consumer skips them — the "container-gap line" navigation already
steps over). Source bytes, not lines, are returned so pampa stays
`SourceContext`-free; the consumer resolves `sourceLine =
byteLineMap.lineOf(source_byte_start) + (L − out_line_start)`. **This makes N1–N3
map correctly.**

### 2.6 Why per-leaf-**block** granularity suffices for **both**

The two projections need different exactness, and per-leaf-block delivers each:

- **Engine (byte-level via `map_offset`)** only ever queries **code** lines. A
  code block is emitted **verbatim**, and in the whole-doc output its `> `/indent
  prefixes are present in *both* source and output, so within its own piece the
  linear `source_start + offset` is **byte-exact**. Prose leaves map only
  approximately at the byte/column level (escaping, delimiter canonicalization) —
  but the engine never reads prose, and that approximation is **no worse than
  today**. The fix is purely the *anchoring* (own piece, not the container's).
- **Nest-in (line-level via line spans)** needs only that a leaf preserves its
  **internal line count** (output line `i` ↔ source line `i`), which holds because
  `write_soft_break` emits a newline (1:1). The single-block buffer strips the
  outer prefix (columns shift) but never changes a leaf's line *count*.

Neither needs per-leaf-**inline** tracking; the escaping/delimiter column
subtleties are out of scope (§10).

---

## 3. Reimplementing `write_with_source_info` — backward-compat

This is the one **existing** method we change; treat it carefully.

- **Signature & callers unchanged.** Audit callers first — primary is
  `engine_execution.rs:511` (`with_source_info`). Grep for every `write_with_source_info`
  use before landing.
- **Tiling changes.** Today each top-level block piece *absorbs* its preceding
  blank separator; the recursive core makes inter-/intra-container glue
  **`Generated`**. Consequence: `map_offset` on a **glue byte** (a separator
  newline, a `:::`/fence line, a trailing newline) now returns **`None`** instead
  of a (previously wrong-anyway) linear guess. For the engine this is strictly
  better — code bytes resolve exactly; glue bytes honestly resolve to "unknown"
  rather than a confident wrong line.
- **Existing test impact.** `engine_execution.rs` has
  `test_source_info_map_offset_single_file` (queries "Body" — a content byte, still
  resolves) and `test_source_info_map_offset_start_and_end` (queries **the last
  byte**, often a trailing-newline = glue → would now be `None`). The latter must
  be updated to query a content byte (it was asserting on a structural byte). Pin
  this in the Test Seam Spec (T-ENG-compat).
- **No `SourceContext` dependency added** — the SourceInfo references the same
  `block.source_info()` file ids as today.

---

## 4. Leaf / container classification (asserted by a test)

| Block | Kind | Note |
|---|---|---|
| `Plain`, `Paragraph`, `Header` | leaf | SoftBreak→newline preserves line count |
| `CodeBlock`, `RawBlock` | leaf | verbatim — the engine-critical exact case |
| `LineBlock`, `HorizontalRule` | leaf | |
| note-def / metadata leaves | leaf | confirm during impl |
| `BlockQuote`, `BulletList`, `OrderedList`, `DefinitionList`, `Div`, `Figure` | container | recurse; not instrumented |
| `Table` | **open decision** | §6.1 |

---

## 5. Open decisions (defaults chosen)

1. **`Table`** — DEFAULT: single leaf span/piece anchored at its source start, with
   a documented best-effort caveat (line 0 exact; interior multi-line interpolation
   may drift). Alternative (exclude → synthesized) rejected as more surprising.
2. **Line-span output shape** — DEFAULT: sparse `Vec<BlockLineSpan>` with source
   **bytes** (pampa stays `SourceContext`-free). Dense per-line array rejected.
3. **Glue → `Generated` (Projection A)** — DEFAULT: yes (honest; `map_offset`
   returns `None` for non-content bytes). Alternative (fold glue into an adjacent
   container piece to keep full byte-mapping) considered for maximal backward-compat
   but rejected: it would re-introduce a small linear drift on glue and obscure the
   "synthesized" signal. We instead update the one test that queried a glue byte.
4. **Whole-doc line-spans variant** — DEFAULT: not now (engine uses SourceInfo;
   nest-in uses the block method). Trivial to add later if a caller wants it.

---

## 6. Test Seam Spec (FROZEN before implementation)

Per `prevalidating-test-seams`: each row names the **real unit** (the new/changed
pampa method, native Rust, **no mocks**), the **seam** (input qmd · call ·
assertion on the returned `(bytes, spans|SourceInfo)`), and the **named revert →
RED**. Tests live in `crates/pampa/tests/integration/` (registered in `main.rs`).
The N*/E* fixtures are the research note's, with their observed-wrong numbers
flipped to **correct**. RED-first.

### Nest-in (Projection B — `write_block_with_line_spans`)

| # | Fixture (research N*) | Assertion surface | Named revert → RED |
|---|---|---|---|
| T-N1 | `> > A / > > / > > / > > B` (nested BlockQuote, double-blank) | the span for `B` resolves to **source line 3** (was Ls=2) | remove the `Paragraph` leaf bracket → no span / wrong anchor → RED |
| T-N2 | `- outer / ⟂ - b / ∅ / ∅ / ⟂ - c` (BulletList sublist, loose) | the span covering `c` resolves to **source line 4** (was Ls=2) | drop the gap→synthesized handling → blank lines counted → wrong line → RED |
| T-N3 | ordered sublist, loose | span for `c` → **source line 4** | same as T-N2 |
| T-N-multiline | a `Paragraph` with internal soft-breaks | one span, `out_line_count>1`, interior lines interpolate to the right source lines | break within-leaf line interpolation → RED |
| T-N-coverage | mixed block | spans ordered, non-overlapping, every non-glue output line covered exactly once | weaken assembly → overlap/hole → RED |
| T-N-parity | any block | `write_block_with_line_spans(b).0 == write_single_block(b)` | any byte change → RED |

### Engine (Projection A — `write_with_source_info`)

| # | Fixture (research E*) | Assertion surface | Named revert → RED |
|---|---|---|---|
| T-E1 | `Div` + prose + collapsing blanks + code (`BOOM`) | `map_offset(byteof BOOM)` resolves to **BOOM's source line** (was the fence line above) | revert recursion (track only top-level blocks) → BOOM inherits the Div's linear piece → wrong line → RED |
| T-E2 | `BlockQuote` + blanks + code | `map_offset(BOOM)` → BOOM's source line | same revert → RED |
| T-E3 | `Div` + `BulletList` + blanks + code | `map_offset(BOOM)` → BOOM's source line | same revert → RED |
| T-E-glue | any nested-code fixture | `map_offset` of a fence/separator byte returns `None` (glue is `Generated`) | fold-glue-into-piece variant → returns Some → RED (pins the §5.3 decision) |
| T-ENG-compat | `# Title\n\nBody…` (existing engine fixture) | `map_offset(byteof "Body")` still resolves to Body's source line; **update** the old "last byte resolves" assertion to a content byte | n/a (compat pin; guards we didn't regress top-level mapping) |
| T-E-tiling | mixed doc | the SourceInfo tiles `[0,len)` with no gaps; leaf pieces are `Original`, glue pieces are `Generated` | weaken assembly → hole/overlap → RED |

### Shared

| # | Assertion | Revert → RED |
|---|---|---|
| T-classify | leaf/container split matches `write_block`'s arms | misclassify a container as leaf → spurious span/piece → RED |
| T-table | §6.1 default: a table yields a single span/piece anchored at its source start (documents the caveat) | n/a (behavior pin) |

**Vacuity guards:** T-N2/T-N3 discriminate on *the collapsed-blank line being
synthesized*, not "a span exists." T-N3 keeps parent/child source lines distinct.
T-E1–E3 discriminate on the *mapped line equalling BOOM's actual line*, with the
"revert recursion" hunk proving the bug returns.

---

## 7. Checklist (TDD order)

- [ ] **Caller audit:** grep every `write_with_source_info` use; confirm the engine
      path is the only consumer; record findings.
- [ ] **Sink + ctx plumbing:** counting bottom-sink adapter + `ctx.out_pos()` /
      `record_leaf` accumulator on `QmdWriterContext` (qmd.rs:39); no-op on the
      untracked path. Build only.
- [ ] **T-N-parity / text-parity** RED→GREEN: stand up `write_block_with_line_spans`
      returning `(bytes, vec![])`; assert byte-parity with `write_single_block`.
- [ ] **T-N1** RED → instrument `Paragraph`/`Plain` leaves → GREEN.
- [ ] **T-N2/T-N3** RED → recurse instrumentation through list/quote containers +
      gap→synthesized assembly → GREEN. *(Headline nest-in bug.)*
- [ ] **T-N-multiline / T-N-coverage** RED→GREEN.
- [ ] **Recursive `SourceInfo` core:** assemble `Concat` (leaf `Original` + glue
      `Generated`); **reimplement `write_with_source_info`** on it.
- [ ] **T-ENG-compat** GREEN first (update the stale last-byte assertion), to prove
      top-level engine mapping is preserved.
- [ ] **T-E1/T-E2/T-E3** RED→GREEN (nested code now maps to the right line).
- [ ] **T-E-glue / T-E-tiling** RED→GREEN.
- [ ] **T-classify / T-table** per §4/§6.1.
- [ ] Full pampa suite: `cargo nextest run -p pampa`.
- [ ] **quarto-core engine tests:** `cargo nextest run -p quarto-core`
      (the `engine_execution` map_offset tests are the live consumers).
- [ ] Workspace regression: `cargo nextest run --workspace`.
- [ ] `cargo xtask verify --skip-hub-build` (`-D warnings`).

---

## 8. Risks / watch-items

- **`write_with_source_info` is a live engine dependency.** The reimplementation
  must keep top-level code-cell mapping exact (T-ENG-compat) while fixing nested
  cells. The only intended behavior change is glue→`None`; everything content
  stays mapped. Audit callers; run quarto-core tests.
- **Within-leaf line-count fidelity** (the one load-bearing assumption): holds for
  `Para`/`Plain`/`CodeBlock`/`Header`. Scrutinize `Table` (§6.1).
- **Marker/prefix rides inside a leaf piece** — intentional (§2.3). For the engine
  it is correct because whole-doc output reproduces those bytes verbatim from
  source; for nest-in it is irrelevant at line granularity. Do not "fix" it.
- **Glue `Generated` changes `map_offset` on structural bytes to `None`** — the one
  observable backward-compat change; the only affected test queries a glue byte and
  is corrected (T-ENG-compat).
- **Columns stay rudimentary** — by design; this plan does not touch caret column
  placement.

---

## 9. Out of scope (recorded)

- Frontend / engine wiring (`regenerate_nested_buffers.rs`, `nestedEditBuffers`,
  `Ls` derivation, engine error-mapping consumers).
- **Column-exact (per-leaf-inline) provenance** — the heavier variant that also
  fixes caret columns and escaped-`Str` byte-exactness. Forward-compatible: same
  counting sink, instrumentation pushed down to inline leaves. Not needed for
  either line-number failure here.
