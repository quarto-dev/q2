# Block editing — Plan 4: section editing (range replace)

**Date:** 2026-06-06
**Branch:** feature/block-editing (worktree `.worktrees/block-editing`)
**Spec:** `claude-notes/designs/2026-06-06-block-editing-design.md`
**Phase:** 4 (final). Rust (`pampa` + WASM) + frontend.
**Depends on:** Plans 1, 2a, 2b, 3.

## Overview

Make whole **sections** editable, and ship the **heading-vs-section geometry**
that the no-pencil interaction model needs. A section is a sectionize-generated
`Div` (`.section`) wrapping a heading + body blocks; its `source_info` is
`Generated{by:sectionize, from:[]}` → **not sliceable**, and it spans **multiple**
untransformed top-level blocks. So (D7): the **frontend** computes the section's
source **envelope** `[min start, max end]` over its `Original` descendants and
sends a **range** edit; the **backend** adds a range lookup + range splice. The
reconcile/writer core is **unchanged** — it already replaces a contiguous N
blocks with M blocks while preserving bytes outside the span (verified in the
spec).

**Win:** edit a whole section (heading + body) at once; nested sections handled
by the envelope.

## Heading-vs-section by geometry (the interaction half)

An `<h2>` is a **full-width block box**, so element hit-testing (`closest()`)
can't tell "on the heading" from "in the blank space to its right." The split
therefore uses **glyph-rect hit-testing**:

- The heading's **inline text rectangle** (`Range.getClientRects()` over the
  heading's text) is the **heading** target → edits the heading (Plan 1/2 path).
- The **section** rectangle showing through everywhere else — notably to the
  **right of the heading text** — is the **section** target → edits the section
  by range.
- **Nested sections** disambiguate naturally: each section's handle is at its own
  heading's row (distinct y); deepest-section-wins for body whitespace.
- **Keyboard** expresses the same split as adjacent **DOM pre-order** roving-
  tabindex stops: the section stop precedes its heading stop
  (`<section>` opens before `<h2>`), so `section → heading → first para → …` falls
  straight out of pre-order. Both stops share the two outline shapes
  (whole-section rect vs. heading-text rect), so keyboard and pointer match.

This is the **first instance of the general "text selects / background activates"
model**; the glyph-rect primitive built here is what later generalizes. The
**whole-container-as-a-unit** edit for non-section `Div`/`BlockQuote`/lists
(shadowed once Plan 3 made their contents editable) reuses this same
background-vs-text mechanism — building it for sections sets it up for containers.

## TDD work items (tests first)

### Rust tests (`crates/pampa/tests/integration/node_edit_tests.rs`)
- [ ] Fixtures: a doc with two sections (h2 + paras); nested sections (h2 → h3);
  a section whose body includes a fenced div.
- [ ] `lookup_range(ast, start, end)` returns the contiguous top-level block span
  `(i, j)` covered by the range; boundary-misaligned ranges → error/`None`.
- [ ] Range edit: replace a section (heading + 3 paras) with 2 blocks (N→M);
  assert bytes **outside** the section are preserved verbatim and the result
  re-parses to the intended structure.
- [ ] Nested-section envelope: editing the outer section replaces the inner
  section's blocks too (they fall in range).
- [ ] **Tier-2 body, snapshotted:** a section whose body contains a list or table
  — editing the section snapshots the reformatted body (Tier-2 blocks within
  reformat); blocks **outside** the section stay verbatim. **Not** byte-identity
  within the section.

### Frontend tests
- [ ] `sectionEnvelope(sectionNode, pool)` util: min/max over `Original`
  descendants; excludes generated descendants; returns `null` when a section has
  no `Original` descendant (→ not editable).
- [ ] **Glyph-rect hit-test:** a point over the heading text → heading target; a
  point on the heading's row but **right of the text** → section target; a point
  in section body whitespace → deepest section. Synthetic `getClientRects()`
  rects (jsdom can't lay out text); true behavior in the browser E2E.
- [ ] **Keyboard pre-order:** arrowing from a section stop reaches its heading
  stop, then its first paragraph; the section and heading stops carry distinct
  accessible names and outline shapes.

### Implementation
- [ ] `crates/pampa/src/node_lookup.rs` — `lookup_range(ast, start, end) ->
  Option<(usize, usize)>` over top-level blocks (first/last whose ranges fall
  within `[start,end]`; reject partial overlap).
- [ ] `crates/pampa/src/apply_node_edit.rs` — range splice `blocks.splice(
  i..=j, new_blocks)`; share the reconcile+write tail.
- [ ] `crates/wasm-quarto-hub-client/src/lib.rs` — new entry
  `apply_node_edit_range(content, untransformed_ast_json, start, end,
  modified_subtree_json)`.
- [ ] `ts-packages/preview-renderer/src/types/diagnostic.ts` —
  `PreviewNodeEditPayload` (already a `channel`-discriminated union from Plan 2b)
  gains a third variant: `{ channel: 'range'; range: [number, number];
  modifiedSubtreeJson: string }` for section edits (no `destinationSourceInfoJson`
  — the range replaces a span of top-level blocks, not a single target).
- [ ] `q2-preview/blocks/Div.tsx` — when `.section` and an envelope exists,
  carry `data-section-range`; the editor uses the range payload.
- [ ] `q2-preview/utils/sectionEnvelope.ts` (new).
- [ ] **Glyph-rect hit-test in `useBlockEditHover`** — extend the Plan 2 hover so
  that, on a heading row, it chooses the heading vs. the enclosing section by
  whether the pointer is inside the heading's inline text rect
  (`Range.getClientRects()`). Add the section as a roving-tabindex stop preceding
  its heading (DOM pre-order). The section/heading outlines are the two shapes
  (section rect / heading-text rect).
- [ ] Parent branch: `ReactPreview.tsx` `handleSetAst` / the `applyNodeEdit`
  service routes a `range` payload to `apply_node_edit_range`; otherwise the
  existing single-target path. (Iframe slices the envelope locally for display.)

## End-to-end verification
- [ ] `cargo nextest run -p pampa` green.
- [ ] `cargo xtask verify` (full — touches `pampa`/WASM) then dev server: edit a
  whole section (change the heading **and** a body paragraph in one box) →
  confirm the `.qmd` reflects both and neighboring sections are untouched.
  Record output.

## Risks / watch-items
- **Section edit re-serializes the whole body (Tier-2, amplified).** A section is
  edited as one box, so committing re-parses and re-serializes **every** block in
  the section body — any list / table / blockquote inside reformats (renumber,
  re-pad, `>` reflow), even parts the user didn't touch, and an unchanged
  resubmit is **not** a no-op. This is the accepted "submit is not a no-op"
  contract (design Edge cases) at section scale. The guarantee that holds: blocks
  **outside** the section stay byte-verbatim.
- **Boundary alignment:** the envelope must align to untransformed block
  boundaries; reject (read-only) if a range bisects a block.
- **Section with only generated content:** no envelope → no affordance (acceptable).
- **Re-sectionizing after edit:** adding/removing a heading re-shapes sections on
  the next render — handled by the normal round-trip; add a test if cheap.

## References
- Spec D7 (+ "writer already supports N→M"); `transforms/sectionize.rs`,
  `node_lookup.rs`, `apply_node_edit.rs`, `writers/incremental.rs`,
  `types/diagnostic.ts`, `q2-preview/blocks/Div.tsx`.
