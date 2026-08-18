# Preview capture-splice as a three-way merge

**Status:** Proposed — **deferred**. Do not implement from this document yet; it
was judged too risky to undertake at the time of writing. The purpose here is to
preserve the design and its reasoning so that a future implementer begins from
the analysis rather than rediscovering it.

**Date:** 2026-07-24

**Related code**
- `crates/quarto-core/src/engine/capture_splice.rs` — the current lock-step walk this design would replace.
- `crates/quarto-ast-reconcile/` — the two-way AST reconciliation this design builds on.
- `crates/quarto-core/src/stage/stages/engine_execution.rs` — the render path, which already uses two-way reconciliation (`reconcile(ast, executed_ast)`).

**Related strands:** bd-lucp (original splice), bd-7hqea3qi (Div recursion),
bd-5jxcio5d (marimo RawBlock), bd-5oyk1xce (Bug B, multi-engine fold),
bd-5m1ni9if (open splice edge).

---

## 1. Why preview needs a splice at all

`q2 preview` shows the reader executed engine output — plots, computed tables,
marimo widgets — while they edit the source. Re-running the engine on every
keystroke is not viable, so the preview runs the engine **once**, server-side,
and records the result as an `EngineCapture`: the triple
`(engine_name, input_qmd, result_markdown)`. Every later edit reuses that one
capture.

Reuse is the whole problem. The moment the reader types a character, the live
source no longer matches the source that was captured, so the recorded output
cannot simply be replayed verbatim. The splice exists to bridge that gap: it
takes the recorded transformation and re-applies it to the edited document.

Three ASTs frame the task. Naming them once here fixes the vocabulary for the
rest of the document:

- **A1** — the captured *pre-engine* AST (`parse(capture.input_qmd)`).
- **B1** — the captured *post-engine* AST (`parse(capture.result_markdown)`).
- **A2** — the *live, edited* pre-engine AST (what the current source produces
  before the engine would run).

The engine turned A1 into B1. The splice must produce **B2** — what the engine
*would* produce for A2 — without running the engine again.

## 2. The current algorithm and its recurring failure

The current splice (`derive_cell_outputs_walk` in `capture_splice.rs`) treats the
capture as a recipe in two steps. First it diffs A1 against B1 to learn "which
output block did the engine emit for each source cell," recording a map keyed by
`(structural_hash(cell), occurrence_index)`. Then it walks A2 and, for each
source cell, swaps in that cell's recorded output.

The diff in the first step is a hand-rolled **lock-step walk**. It advances two
pointers through A1 and B1 in parallel under three assumptions: prose blocks
appear identically in both and advance both pointers; each engine cell in A1 maps
to exactly **one** "engine-output block" in B1 (a `.cell` wrapper Div, or a
marimo island `RawBlock`); and any divergence from this pattern stops the walk
(the *fail-soft* rule — whatever was collected before the divergence stays valid,
and everything after falls through to raw source).

That "exactly one output block per cell" assumption is the recurring fault line.
Real engines violate it in a new way every few months, and each violation has
arrived as a silent, output-dropping bug with no error anywhere:

- **bd-7hqea3qi** — a figure-labelled cell is *nested* in a float Div, so the
  walk hit two unequal Divs and stopped. Fix: recurse into Div content.
- **bd-5jxcio5d** — marimo emits islands as bare `RawBlock`s, not `.cell` Divs,
  so nothing matched. Fix: widen the output-block predicate to accept `RawBlock`.
- **bd-5oyk1xce (Bug B)** — a foreign engine's un-executed cell stalled the B1
  pointer and derailed the walk. Fix: advance past a structurally-equal
  passthrough block.
- **The bug that prompted this document** — an `echo: true` marimo cell emits
  **two** sibling blocks (an echoed-source `CodeBlock`, then the island). The
  echoed `CodeBlock` is not an output block, so the walk breaks at the first such
  cell and drops every cell after it.

Each fix is correct and well-tested. Together they are a symptom: the model
underlying the walk is weaker than the output real engines produce, so the model
accretes special cases instead of generalizing.

## 3. The insight: render already reconciles; preview is the three-way case

The render pipeline does not use this walk. After the engine runs, render calls
`quarto_ast_reconcile::reconcile(ast, executed_ast)` — a general, content-hash
two-way reconciliation — to merge the pre-engine and post-engine ASTs while
preserving source locations. That path renders `index.qmd`'s six marimo islands
correctly. The preview path, using the bespoke walk on the *same* capture, drops
five of them. The reconciliation machinery is the part that works; the bespoke
walk is the anomaly.

The reason preview forked away from reconciliation is real, not accidental.
`reconcile` is **two-way**: it merges one before-AST with one after-AST for the
*same* document version. Preview is **three-way**: it must combine the recorded
transformation (A1→B1) with a *different*, edited document (A2). A two-way merge
does not directly express that.

The design in this document closes the gap by building the three-way merge **on
top of** the two-way primitive, rather than hand-rolling a diff beneath it.

## 4. The design: a three-way merge over blocks

A three-way merge (the classic `diff3`) needs two diffs against a shared base.
Here the base is A1, and the two-way reconciliation supplies both diffs:

- the **engine diff**, `reconcile(A1, B1)` — which B1 blocks are unchanged prose
  and which are new engine output;
- the **user diff**, `reconcile(A1, A2)` — which A2 blocks the reader left
  untouched and which they edited.

The two-way primitive reports each alignment as `KeepBefore` (content-identical
to a base block — a hash match), `UseAfter` (new or changed content), or
`RecurseIntoContainer` (same container, descend). From those two alignments the
merge finds the base blocks matched in *both* diffs — the stable anchors — and
classifies each chunk between consecutive anchors by how its A2 (the reader's)
and B1 (the engine's) ranges relate to the base:

| A2 vs. base | B1 vs. base | result |
|---|---|---|
| unchanged | changed | take **B1** (splice the engine output) |
| changed | unchanged | take **A2** (edited cell falls through to raw source) |
| unchanged | unchanged | keep the base |
| changed | changed | conflict → take **A2** (raw source) |

This table earns its keep by *deriving* today's behavior instead of hand-coding
it. An unedited cell shows engine output; a cell the reader is actively editing
shows raw source until the next capture — exactly the current contract, now a
consequence of the merge rather than a special case inside a walk.

## 5. Cell-order attribution: the one piece the merge does not give for free

Block-level `diff3` handles a single expanded cell cleanly. For an `echo: true`
cell the base chunk is `[cell]`, the reader's chunk is the unchanged `[cell]`,
and the engine's chunk is `[echoCode, island]`; the table says "take the
engine's chunk," and both blocks splice with no per-shape knowledge required. The
entire family of output-block predicates disappears.

Adjacent cells with a partial edit are the case the merge cannot resolve on its
own, and the reason is fundamental: **the engine erases cell identity.** A cell
and the island it becomes share no content, so a content diff finds *no anchor*
inside a run of adjacent cells. Consider `index.qmd`'s first two cells, which sit
together with no prose between them, when the reader edits only the second:

```
base A1   = [ cell1, cell2 ]
live A2   = [ cell1, cell2' ]           (only cell2 edited)
engine B1 = [ island1, echoCode2, island2 ]
```

The base↔engine diff anchors nothing here — `cell1` and `cell2` both vanished
into unrelated output — so `diff3` sees one chunk changed on both sides,
declares a conflict, and takes the reader's side: `[cell1, cell2']`. That drops
`cell1`'s island even though the reader never touched `cell1`. The result is
worse than the bug being fixed.

The resolution keeps the merge but adds one assumption that holds for every
execution engine: **the engine emits output in cell order and touches only
cells.** Within an engine-changed region, attribute the output run to source
cells by order — cell *k* owns the run up to where cell *k+1*'s output begins —
producing a per-cell output run. The "did the reader edit this cell" test then
runs per cell, on the hash of the cell's source, which is precisely today's
`(hash, occurrence)` key. In the example, `cell1` (unedited) takes `island1`,
`cell2` (edited, hash miss) falls through to `cell2'`, and `echoCode2`/`island2`
are attributed to `cell2` and dropped as stale — yielding `[island1, cell2']`,
which is correct.

The attribution is the one place engine-specific reasoning can re-enter, because
splitting a multi-block run across adjacent cells needs a *cell-boundary* signal
(see the open questions). The design's aim is to confine that reasoning to a
single, explicit place instead of spreading it across a growing predicate.

## 6. Why this is more general and less fiddly

The current walk carries a table of shapes it must recognize — `is_cell_wrapper`,
`is_engine_output_block`, the `RawBlock` special case, the passthrough rule — and
every new engine output shape adds a row. The three-way merge removes that table.
It rests instead on one assumption that is true of every engine we support: prose
passes through untouched, and cell outputs are emitted in cell order. The remaining
hard case — adjacent cells under a partial edit — is handled once, by order
attribution, rather than re-litigated per engine.

## 7. Open design questions

The merge trades a growing list of per-shape patches for a small set of sharper
questions. These are unresolved and would need answers before implementation:

1. **Cell-boundary delimitation.** Order tells us cell *k* precedes cell *k+1*,
   but not where a multi-block run splits between them. The available signals
   (`.cell` Div per cell, one island per cell) are engine-specific — the very
   knowledge the merge set out to remove. The honest question is whether we can
   avoid a per-engine boundary abstraction, or should instead shrink the
   engine-specific surface to one declared contract ("emit one recognizable
   boundary per cell").

2. **Fall-through granularity: per cell or per region.** If we decline to solve
   (1), the simple alternative reverts an entire adjacent-cell region to raw
   source when any cell in it is edited. Coarser, fully engine-agnostic, and
   possibly fine for a preview that re-captures on save. This is a product
   decision, not a technical one.

3. **Order attribution vs. content keying under reordering.** Cells match by
   position-independent `(hash, occurrence)`, but output attribution is
   order-based. Reordering two unedited cells can make the two disagree. The
   design needs one coherent rule.

4. **Is prose actually invariant?** `results='asis'`, inline execution, and
   markdown-emitting cells (`mo.md()`) rewrite prose, shrinking the anchor set.
   The design needs an explicit definition of "anchor" (likely: only
   hash-identical-across-A1↔B1 blocks) and a stated behavior for these engines.

5. **Three-way recursion into containers.** A `::: {#fig-…}` float wraps the raw
   cell in A1 and the executed output in B1 — same container, changed content.
   The merge needs a rule for when to descend and merge children versus treat the
   whole container as one changed chunk, keeping the occurrence counter in
   document order across nesting levels.

6. **Multi-engine fold.** Captures fold in sequence; engine 2's base is engine
   1's spliced output. Whether sequential three-way merges compose correctly, or
   interleaved output needs a joint attribution, must be first-class — Bug B lived
   in exactly this seam.

7. **Fail-soft floor.** Today's walk cannot emit wrong output; its worst case is
   raw source. A merge can be confidently wrong — stale output spliced onto a
   reverted cell, or mis-attributed across adjacent cells. Stale-but-plausible
   output is arguably worse than visibly-raw source, so the design should keep a
   guard: splice only when attribution is unambiguous, else fall through.

8. **Cost and caching.** The merge runs in WASM on roughly every edit and costs
   two reconciliations plus classification, against today's single O(n) walk. The
   engine diff (A1→B1) is fixed per capture; only the user diff (A1→A2) changes
   per keystroke, so the engine side should be cached per capture from the start.

9. **The reframing question — attribute at capture time, not splice time.** Every
   question above is a consequence of one earlier decision: the capture stores a
   flat markdown blob, discarding the cell→output correspondence the engine knew
   exactly at execution time. If the capture instead recorded structured per-cell
   output (`Vec<(cell_key, output_blocks)>`), the browser splice would collapse to
   a keyed lookup per A2 cell, and questions 1–7 would not arise. The cost moves
   into the engine-host capture contract. This is the sharpest fork: a principled
   inference engine in the browser, versus recording what the engine already knew.
   It should be settled before the merge is built, because it may make the merge
   unnecessary.

## 8. Risk and why this is deferred

The current walk, for all its accreted patches, has a strong safety property: it
never emits wrong output. Replacing it with a merge introduces the possibility of
confident mis-attribution (question 7), touches a component that runs on every
keystroke in WASM (question 8), and interacts with the multi-engine fold that has
already produced one subtle bug (question 6). The reframing question (question 9)
may also redirect the whole effort toward the capture contract instead. Given
that the immediate bug has a small, contained fix that stays inside the current
model (see the companion discussion), the merge is recorded here and deferred
rather than started now.
