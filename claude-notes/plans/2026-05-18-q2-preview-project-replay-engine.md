# q2 preview: AST-splice the captured engine output into live edits

**Beads:** bd-lucp (parent-child to bd-kw93, discovered-from bd-m0mu).
**Supersedes:** bd-m0mu (engine_registry plumbing) — see "Why this
supersedes bd-m0mu" below.
**Status:** Design (2026-05-18). Implementation pending.

## Goal

Make `q2 preview` actually show server-side engine output in the iframe
preview, *across edits to the live `.qmd`*. Today (without this work)
the SPA receives the recorded `EngineCapture` but the WASM-side
pipeline can't use it: the original Phase C.4 design routed capture
bytes through `EngineRegistry::with_replay`, which uses byte-equality
against the recorded `input_qmd`. Any prose edit (or mtime drift from
`listing-item.date-modified`) misses, and the user sees raw `{r}`
source instead of `cat("Hello, world")`'s output.

## What changed in the design (2026-05-18 review)

The 2026-05-11 epic and the 2026-05-13 Phase C plan both routed
preview-time use of captures through the existing `ReplayEngine` as a
drop-in replacement inside `EngineExecutionStage`. That is the right
shape for `ReplayEngine`'s *own* consumer (the bd-45yw regression-
testing tool, where hard byte-equality is correct and required), but
it is the wrong shape for *preview*. Preview's input is live-edited;
the QMD reaching `EngineExecutionStage` is, by design, never byte-
identical to what was captured a few seconds ago. The hard-miss
contract is correct for `ReplayEngine`'s users and wrong for preview's.

**The right pattern:** preview consumes the capture as an *AST-level
transformation recipe*. The capture tells us: "when the engine
executed v1, it transformed v1's pre-engine AST into v1's post-engine
AST in such-and-such a way." At preview render time for v2, we run v2
up to the pre-engine checkpoint, then splice the captured AST-level
transformation onto v2's pre-engine AST — **without invoking
`EngineExecutionStage` (or `ReplayEngine`) at all**. Code cells with
unchanged content get their captured output spliced in; code cells
whose content changed in v2 (or that didn't exist in v1) fall through
to today's raw-source rendering, the same as the no-capture path.

`ReplayEngine` itself is untouched. It remains exactly what bd-45yw
designed: a deterministic, hard-miss, regression-testing tool.
Preview's use of capture bytes goes through a *different* code path.

## Reproduction (unchanged from the bd-m0mu report)

`~/Desktop/daily-log/2026/05/15/q2-preview-test-website/` — a 2-page
website with one R cell calling `cat("Hello, world")`. With `q2
render`, the rendered HTML wraps the cell in `<div class="cell">` with
a `<div class="cell-output cell-output-stdout">` sibling. With `q2
preview` (today, no fix), the iframe shows raw `<pre><code
class="{r}">cat("Hello, world")</code></pre>` and no output sibling.

Server-side capture is fine on every run (verified end-to-end on
2026-05-18 with `RUST_LOG=quarto_preview=debug` plus inspection of the
gzipped capture binary doc at `<data_dir>/captures/<sha>.bin`). All
three failure layers below are on the *consumer* side.

## Failure layers (from outer to inner)

1. **bd-m0mu** (was: project-pipeline drops engine_registry). Fix
   reverted in favor of the splice design; the registry plumbing was
   needed *only* for the now-discarded "preview uses ReplayEngine"
   architecture.
2. **bd-4uvv** (samod TS `repo.find()` requires `automerge:` URL
   prefix; `getBinaryDocById` was passing bare docId → silent
   `Invalid AutomergeUrl` → capture bytes never reached the WASM).
   **Fix stands** — the splice path still needs the bytes to arrive.
3. **bd-lucp** (the design conflict described above) — the work this
   plan now owns.

bd-m0mu and bd-lucp address the same user-visible symptom; bd-lucp is
the right fix. bd-4uvv is independent and necessary regardless.

## Architecture: cell-targeted splice

### Capture wire format — unchanged

`EngineCapture { engine_name, input_qmd: text, result.markdown: text }`
stays as is. The preview consumer derives ASTs by re-parsing both
sides on the fly. Re-parse cost is small (the strings are short by the
time they're in the capture); the wire-format simplicity is worth it
and matches how engine execution semantically works in Q2 anyway
(engine input/output is text, the pipeline is AST).

### Preview-time flow

For the active file `v2` with capture `cap = (input_qmd_v1,
result_md_v1)`:

```
A1 ← parse(input_qmd_v1)         // v1's pre-engine AST
B1 ← parse(result_md_v1)         // v1's post-engine AST (what the engine produced)

A2 ← v2 through the q2-preview pipeline UP TO the pre-engine
     checkpoint                  // v2's pre-engine AST

map ← derive_cell_outputs(A1, B1)
                                 // (cell_hash, occurrence) → output Blocks

B2 ← splice(A2, map)             // for each engine cell in A2, look up
                                 // its (hash, occurrence) key; if found,
                                 // replace with mapped output Blocks;
                                 // else keep the cell as-is.

continue the q2-preview pipeline from B2 (skipping EngineExecutionStage)
```

### Key shape — `(content_hash, occurrence_index)`

Per the 2026-05-18 design discussion: a few real documents have
repeated identical code cells (e.g. `cat("hello")` twice for testing,
deliberately-redundant boilerplate). A hash of the cell content alone
would alias them. The disambiguator is the cell's occurrence index
among same-hash cells in document order:

- `content_hash`: `quarto_ast_reconcile::compute_block_hash_fresh` on
  the `CodeBlock` node (includes attributes + body — same key shape
  the reconciler uses for matching).
- `occurrence_index`: 0 for the first cell with this hash in document
  order, 1 for the second, …

Both `derive_cell_outputs` (walking A1) and `splice` (walking A2) use
the same `(hash, occurrence)` keying.

### Output extraction — derive_cell_outputs(A1, B1)

Walks `A1.blocks` and `B1.blocks` in parallel. Non-engine-cell blocks
are assumed to appear identically in both (the engine doesn't reorder
or transform prose). Engine cells in A1 correspond to contiguous runs
of output blocks in B1 — the engine produces zero or more output
blocks per cell (a `Div.cell` wrapper containing the source code-block
+ one or more `Div.cell-output-*` siblings is the typical shape).

```
i, j ← 0, 0
seen ← empty counter
output_map ← empty
while i < len(A1.blocks):
    block = A1.blocks[i]
    if is_engine_cell(block):
        key = (compute_block_hash_fresh(block), seen[hash]++)
        # Collect B1 blocks until the next prose match against A1.
        next_prose_i = next non-cell index in A1 after i
        run = []
        while j < len(B1.blocks) and (next_prose_i is None or
              !structural_eq_block(B1.blocks[j], A1.blocks[next_prose_i])):
            run.push(B1.blocks[j]); j++
        output_map[key] = run
        i++
    else:
        // Prose block; advance both pointers.
        assert structural_eq_block(A1.blocks[i], B1.blocks[j])
        i++; j++
```

If the parallel walk diverges in an unexpected way (capture has been
corrupted, or the engine emitted prose-affecting filters), fall
through cleanly: emit no map entries for the divergent region. The
splice then leaves those cells as raw source.

### Splice — splice(A2, output_map)

```
result ← []
seen ← empty counter
for block in A2.blocks:
    if is_engine_cell(block):
        key = (compute_block_hash_fresh(block), seen[hash]++)
        if key in output_map:
            result.extend(output_map[key])
        else:
            // No match (cell content changed, cell added, or A1 didn't
            // have a cell at this index with this hash). Leave the
            // cell as raw source — same as today's no-capture path.
            result.push(block)
    else:
        result.push(block)
return Pandoc { blocks: result, meta: A2.meta, ... }
```

### `is_engine_cell` predicate

A `CodeBlock` whose attribute classes include `{<engine-name>}` —
e.g. `{r}`, `{python}`. The pre-engine AST stores these literally
(verified 2026-05-18 against the running pipeline — we saw `<code
class="{r}">` in the iframe DOM). The engine-name check is against
the `engine_name` field of the capture itself, so we only attempt to
splice cells belonging to the captured engine.

### Pre-engine timing — why a flat walk is safe

Both `derive_cell_outputs(A1, B1)` and `splice(A2, output_map)`
above iterate `.blocks` flatly. This is correct **only because the
splice runs at the pre-engine checkpoint** — strictly before
`SectionizeTransform` (and the other "sugar phase" synthesizers)
add the top-level transparent wrapper Div that the writer learned
about the hard way in commits `bdcfdc53` / `b9f64b56` / `2bf92664`.
At the pre-engine checkpoint, `A2.blocks[0]` is a real user block.

If a future variant ever moves the splice point past the sugar
phase (or runs it on a post-pipeline AST for any other reason),
the flat walk would miss every cell inside the wrapper. Route the
walker through `first_in_user_tree` / a `visit_user_blocks`
sibling per
[`claude-notes/designs/transparent-wrappers.md`](../designs/transparent-wrappers.md)
in that case.

## Where the splice lives in the pipeline

Two viable insertion points; the v1 picks the simpler:

### v1: `CaptureSpliceStage` replaces `EngineExecutionStage`

When a capture is attached to the pipeline config, the q2-preview
pipeline builder swaps `EngineExecutionStage` for a new
`CaptureSpliceStage`. The latter:

1. Receives the same `Pandoc` input `EngineExecutionStage` would have
   received (the pre-engine AST after pre-engine sugaring + metadata
   merge).
2. Re-parses `capture.input_qmd` → A1 and `capture.result_markdown`
   → B1 via the existing pampa parser. (Parse is cheap.)
3. Computes `output_map` from `(A1, B1)`.
4. Walks the current AST and applies the splice.
5. Hands the spliced AST to the rest of the pipeline.

The rest of the q2-preview pipeline runs unchanged. Post-engine
stages see an AST that looks "as if the engine had run", because the
spliced output blocks are exactly what the engine produced server-
side.

`build_q2_preview_pipeline_stages(engine_registry, capture)` becomes
the new signature. When `capture` is `Some`, the engine-stage slot is
filled with `CaptureSpliceStage`; otherwise `EngineExecutionStage`
(today's behaviour, which in WASM falls through with raw source).

### Why not v2: leave EngineExecutionStage in, add CaptureSpliceStage in front

Could work — after splicing, the AST has no `CodeBlock`-with-engine-
class nodes (they're replaced by `Div.cell` output blocks), so
`EngineExecutionStage` would no-op. But it's more fragile: any future
change to engine-detection in `EngineExecutionStage` could
accidentally re-fire on the spliced output. v1's swap is more honest
about the contract.

## Why this supersedes bd-m0mu

bd-m0mu added `engine_registry: Option<EngineRegistry>` to
`RenderToPreviewAstRenderer` so the WASM-side project pipeline could
substitute `ReplayEngine` for the captured engine. Under the splice
architecture, **the preview path never goes through `ReplayEngine` at
all**. The registry plumbing is dead code from preview's perspective.

It could plausibly serve other consumers (a hypothetical native CLI
project-mode replay path, e.g. `q2 render --replay`), but native
project-mode replay isn't a tracked use case; the existing native
single-doc `--replay` path on `q2 render` already covers the
regression-testing use case bd-45yw was designed for. Easier to revert
bd-m0mu now and re-add a (possibly different-shaped) registry hook
later if a real consumer materializes than to carry unused plumbing.

**bd-m0mu changes to revert:**
- `crates/quarto-core/src/project/pass2_renderer.rs` — restore
  pre-bd-m0mu `RenderToPreviewAstRenderer` (no `engine_registry`
  field, no `with_engine_registry` builder, hardcoded `None` in
  `render_qmd_to_preview_ast(.., None)` — yes, this is the original
  "broken" line, but it's no longer wrong because preview doesn't
  consume captures through this path anymore).
- `crates/wasm-quarto-hub-client/src/lib.rs` — restore the
  `_engine_registry` underscore prefix on
  `render_project_active_page_to_response`. The known-gap comment
  remains accurate-for-history: the gap is *now* "preview doesn't
  consume captures here; splice path handles it elsewhere", which a
  one-line addition can capture.
- `crates/quarto-core/tests/render_page_in_project.rs` — delete the
  `project_mode_q2_preview_uses_replay_registry_from_renderer` test
  (which exists only to lock in the registry plumbing this design
  discards). The TDD test for the splice path replaces it.

**bd-4uvv changes stay:**
- `ts-packages/quarto-sync-client/src/client.ts` —
  `automerge:`-prefix normalization + `String(docId)` coercion in
  `getBinaryDocById`. Needed regardless of which downstream consumer
  reads the bytes.
- `ts-packages/quarto-sync-client/src/client.test.ts` — the two
  prefix-normalization regression tests.

## TDD plan

### Phase 1 — Failing splice unit tests (native, fast)

In a new module `crates/quarto-core/src/engine/capture_splice.rs`,
build the splice algorithm with **hand-constructed Pandoc ASTs** (not
parsed from QMD) so the tests don't depend on parser stability and
run in microseconds.

Test cases:

1. **Single cell, unchanged content** — A1 has one `CodeBlock {r}
   "cat('hi')"`, B1 has the post-engine `Div.cell { CodeBlock + Div
   .cell-output-stdout }` for it; A2 is byte-equal to A1. Splice
   produces an AST whose i-th block is the post-engine output.
2. **Single cell, prose edited around it** — A2 = A1 with an extra
   paragraph inserted before the cell. Splice still substitutes the
   cell.
3. **Repeated cells, same content** — A1 has two `CodeBlock {r}
   "cat('hi')"` cells emitting different captured outputs. A2 keeps
   both. Splice substitutes the right output per occurrence index
   (cell 0 → output 0, cell 1 → output 1). **This is the regression
   test for the user's note about repeated cells.**
4. **Cell content changed in A2** — A1 has cell `X`, A2 has cell `X'`
   with different body. Splice leaves the cell as raw source (no map
   entry for `hash(X')`).
5. **Cell deleted in A2** — A1 had a cell, A2 doesn't. Splice
   silently drops the captured output (no place to put it).
6. **Cell added in A2** — A2 has a cell A1 didn't. Splice leaves it
   as raw source.
7. **Wrong engine** — capture is `engine_name = "knitr"`, but A2 has
   a `{python}` cell. Splice doesn't touch it (engine mismatch).
8. **Empty capture (no cells in A1)** — A1 was prose-only; A2 has
   prose-only edits. Splice is a no-op.
9. **Walk divergence guard** — A1 and B1 disagree on a prose block
   (corrupt capture). Splice falls through cleanly; no panic; no
   nonsense output. Cells in A2 render as raw source.

All cases use `quarto_ast_reconcile::compute_block_hash_fresh` and
`structural_eq_block` as the primitives.

### Phase 2 — Pipeline integration test

`crates/quarto-core/tests/render_page_in_project.rs` (or a new
`capture_splice_pipeline.rs`):

10. **Probe-then-splice (replaces the deleted bd-m0mu test).** Set
    up a website project with a recorded capture (hand-authored
    `EngineCapture`); drive
    `ProjectPipeline<RenderToPreviewAstRenderer>` with the capture
    attached; assert the rendered AST JSON contains the captured
    output marker.
11. **Live-edit splice survives prose change.** Same project, but
    after recording the capture, modify the QMD source's prose
    (without touching the code cell). Re-render. The captured engine
    output still appears in the AST.

### Phase 3 — WASM + E2E

12. WASM `render_page_for_preview` already accepts `capture_gz_json`
    (Phase C.4 surface). The change is internal: the WASM no longer
    builds an `EngineRegistry::with_replay` from the bytes; instead
    it deserializes the `EngineCapture` and threads it into the
    pipeline's capture slot.
13. End-to-end browser verification per CLAUDE.md: run `q2 preview`
    on the fixture website, inspect iframe DOM for
    `.cell-output-stdout` containing "Hello, world", record the
    snippet.

## Out of scope (for this issue)

- **Smarter retargeting** for cells that moved positions in A2 (e.g.
  a cell physically relocated within the doc). v1's strategy is
  "structural-hash + occurrence-index matches at any position";
  position itself isn't part of the key, so simple reorderings still
  match. The brittle case is "user edited cell content + added a
  different cell with the old content" — splice picks the old
  capture for the new cell. Documented limitation; acceptable for
  v1.
- **Filter/include handling.** Engines can emit Pandoc filter
  directives or include-in-header bytes. The splice today places
  output blocks inline; filter directives don't fit cleanly. Likely
  fine for v1 fixtures (knitr/jupyter+passthrough); needs a follow-
  up issue if a real fixture trips it.
- **Cross-document captures.** Each doc has its own capture; this
  plan handles per-doc only.

## Risks

1. **Parse divergence between server's `compute_input_qmd` (native)
   and WASM's parser.** The capture's `input_qmd` is serialized by
   the server's QMD writer; WASM re-parses with pampa. Round-trip
   should be lossless, but if a token escape differs the structural
   hashes won't match and every cell falls through to raw source.
   Mitigation: a round-trip test in Phase 1 that parses
   `input_qmd` and asserts the cells' structural hashes equal
   `A2`'s for an unedited document.
2. **Block-level walk assumption.** The algorithm assumes the engine
   transforms only at the block level. True for knitr/jupyter today
   in Q2; would break if an engine emitted inline-only transforms.
   Not a real risk for v1.
3. **Capture/render path divergence.** If the server's pre-engine
   stages differ from WASM's (e.g., a server-only metadata-merge
   stage adds a key WASM doesn't), the `input_qmd` recorded server-
   side may not match what WASM produces when it parses `A2` from
   the same source. **This is exactly the bug bd-45yw's strict
   ReplayEngine surfaces; under splice it's a soft failure** (cells
   fall through to raw source). Worth fixing structurally, but no
   longer blocking.

## Files of interest

Production code:

- `crates/quarto-core/src/engine/` — sibling slot for new module
  `capture_splice.rs`.
- `crates/quarto-core/src/engine/preview_record.rs` — capture-side
  (unchanged; just reread to confirm `input_qmd` shape).
- `crates/quarto-core/src/engine/replay.rs` — left alone.
- `crates/quarto-core/src/pipeline.rs:351` —
  `build_q2_preview_pipeline_stages(engine_registry: Option<…>)`
  gains a `capture: Option<EngineCapture>` parameter (or moves
  capture-vs-registry to a single enum).
- `crates/quarto-core/src/project/pass2_renderer.rs:539` —
  `RenderToPreviewAstRenderer::new()` gains a
  `with_capture(EngineCapture)` builder; the renderer threads the
  capture into the pipeline-stages call.
- `crates/wasm-quarto-hub-client/src/lib.rs:1191` —
  `render_page_for_preview` deserializes `capture_gz_json` into an
  `EngineCapture` (not a `ReplayEngine` registry) and attaches it to
  the renderer.

Reused primitives:

- `crates/quarto-ast-reconcile/src/hash.rs` —
  `compute_block_hash_fresh`, `structural_eq_block`.
- `pampa::parse_qmd_to_pandoc` (or whatever the canonical entry
  point is) — already used by `compute_input_qmd`.

Removed:

- bd-m0mu's `engine_registry`-on-renderer plumbing.

## Phase plan (TDD)

- [ ] Update bd-m0mu beads issue: status `closed`, reason
  "superseded by bd-lucp" — keep history visible.
- [ ] Revert bd-m0mu Rust changes (pass2_renderer, wasm-quarto-hub-
  client lib, the new test).
- [ ] Confirm post-revert `cargo xtask verify` green (no regressions
  from the revert).
- [ ] Phase 1: `capture_splice.rs` module + 9 unit tests. RED →
  GREEN per case.
- [ ] Phase 2: 2 pipeline integration tests. Same flow.
- [ ] Phase 3: WASM wiring. The TS surface
  (`renderPageForPreview(path, userGrammars?, captureGzJson?)`) is
  unchanged.
- [ ] Phase 3: E2E browser verification. Snippet recorded here +
  in commit body.
- [ ] `cargo xtask verify` all 12 steps green.

## End-to-end expectation (success criterion)

After this lands, against the fixture website at
`~/Desktop/daily-log/2026/05/15/q2-preview-test-website/`, with
`q2 preview .`:

- The iframe at `q2-preview.html` shows
  `<div class="cell"><pre class="sourceCode r cell-code">…</pre>
  <div class="cell-output cell-output-stdout"><pre><code>Hello,
  world</code></pre></div></div>` for the R cell.
- Editing the prose in `index.qmd` re-renders without losing the
  captured output (the splice runs against the live-edited AST).
- Editing the cell body invalidates the splice for that cell only;
  the cell renders as raw source until the user requests
  re-execution (the existing C.5 staleness UX).
