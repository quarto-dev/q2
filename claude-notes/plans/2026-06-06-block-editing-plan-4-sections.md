# Block editing — Plan 4: section editing (range replace)

**Date:** 2026-06-06
**Branch:** feature/block-editing (worktree `.worktrees/block-editing`)
**Spec:** `claude-notes/designs/2026-06-06-block-editing-design.md`
**Phase:** 4 (final). Rust (`pampa` + WASM) + frontend.
**Depends on:** Plans 1, 2a, 2b, 3.

> **Review-applied (2026-06-09), as a follow-on to the Plan 3 review.** Three
> changes: (1) the **whole-container-as-a-unit** affordance for non-section
> containers — which Plan 3 *shadows* — is now an explicit deliverable here, not
> just prose (it was promised in §geometry but had no work item); (2) the new
> `apply_node_edit_range` path must carry the post-2b contract (lenient subtree
> reader + `s`/`a` stripping); (3) the "outside the section" guarantee is
> qualified with "and its boundary separators" (OQ2). See
> `2026-06-08-plan-3-review.md` → "Follow-on effects".

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
by the envelope. **Also closes the Plan 3 shadow:** the same geometry makes a
non-section `Div`/`BlockQuote`/list/callout reachable *as a unit* again (e.g. to
edit a fenced div's attributes), which Plan 3 shadowed by making the contents the
deepest target.

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
model**; the glyph-rect primitive built here is what later generalizes.

### Whole-container-as-a-unit (un-shadowing Plan 3) — delivered here

Plan 3 made a container's *contents* editable, so `closest('[data-block-pool-id]')`
now resolves to the **inner** block and the container-as-a-whole affordance is
**shadowed**. This phase un-shadows it with the **same** background-vs-text rule:
when the pointer (or roving-tabindex focus) is over a container's own box but
**not** over any child block's box — the padding/margin gutter, the bullet
column, the `>` quote rail — the **container** wins; over a child block, the
child wins.

**Crucially this needs no new backend.** A non-section `Div`/`BlockQuote`/list/
callout is a single source-backed block that already carries its own
`data-block-pool-id` (editable-as-whole since Plan 2b) and commits through the
existing single-target `text`/`subtree` channel — nested ones via Plan 3's
path-resolved lookup. So the container case is **purely the affordance geometry**:
the hit-test must stop preferring the deepest `data-block-pool-id` when the point
falls in container background. (Contrast sections, which *do* need the new
`range` payload because a section is a `Generated` Div spanning multiple
untransformed blocks.) Building the section geometry first sets this up; the
container case is then a small generalization of the same hit-test.

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
- [ ] **Whole-container-as-a-unit hit-test (un-shadowing Plan 3):** a point over a
  fenced div's padding gutter (not over any child block) resolves to the
  **container's** `data-block-pool-id`; a point over the inner paragraph resolves
  to the **inner** block. Same for a `BlockQuote` rail and a list's bullet column.
  Synthetic rects in RTL; true layout in the browser E2E. (No backend change —
  the container commits through the existing single-target channel.)

### Implementation
- [ ] `crates/pampa/src/node_lookup.rs` — `lookup_range(ast, start, end) ->
  Option<(usize, usize)>` over **top-level** blocks (first/last whose ranges fall
  within `[start,end]`; reject partial overlap). Top-level is correct and
  sufficient: `A_u` is the *pre-pipeline* AST, so it contains **no** section Divs
  (sectionize is a transform) — a section's envelope always maps to a contiguous
  run of top-level untransformed blocks, even for nested `h2 → h3` sections (flat
  `h2, p, h3, p …` in `A_u`). This is disjoint from Plan 3's `NodePath` (nested
  *single* block); the two share only the reconcile+write tail.
- [ ] `crates/pampa/src/apply_node_edit.rs` — range splice `blocks.splice(
  i..=j, new_blocks)`; share the reconcile+write tail. **Mirror the post-2b
  contract:** read the replacement subtree with the lenient
  `read_completing_source_info(.., By{kind:"direct-write"})` (as the single-target
  path does since `f6448afe`), and use typed `By::` constructors — **not**
  deprecated `SourceInfo::default()`.
- [ ] `crates/wasm-quarto-hub-client/src/lib.rs` — new entry
  `apply_node_edit_range(content, untransformed_ast_json, start, end,
  modified_subtree_json)`.
- [ ] `ts-packages/preview-renderer/src/types/diagnostic.ts` —
  `PreviewNodeEditPayload` (already a `channel`-discriminated union from Plan 2b)
  gains a third variant: `{ channel: 'range'; range: [number, number];
  modifiedSubtreeJson: string }` for section edits (no `destinationSourceInfoJson`
  — the range replaces a span of top-level blocks, not a single target). Build
  `modifiedSubtreeJson` through the **same `stripSourceInfoFields` (`s` + `a`)**
  helper the `subtree` channel uses, so the lenient backend reader backfills
  `DirectWrite` provenance.
- [ ] `q2-preview/blocks/Div.tsx` — when `.section` and an envelope exists,
  carry `data-section-range`; the editor uses the range payload.
- [ ] `q2-preview/utils/sectionEnvelope.ts` (new).
- [ ] **Glyph-rect hit-test in `useBlockEditHover`** — extend the Plan 2 hover so
  that, on a heading row, it chooses the heading vs. the enclosing section by
  whether the pointer is inside the heading's inline text rect
  (`Range.getClientRects()`). Also extend the `closest()` query from
  `'[data-block-pool-id]'` to `'[data-block-pool-id], [data-section-range]'` so
  that pointer events over section regions (which carry `data-section-range`, not
  `data-block-pool-id`) are found by the delegated handler. Add the section as a
  roving-tabindex stop preceding its heading (DOM pre-order). The section/heading
  outlines are the two shapes (section rect / heading-text rect).
- [ ] **Whole-container-as-a-unit hit-test (un-shadowing Plan 3)** — generalize the
  same background-vs-text rule to non-section containers: when the deepest
  `data-block-pool-id` under the pointer is a child *but the point lies in the
  enclosing container's own box outside every child block's box* (padding gutter /
  bullet column / `>` rail), resolve to the **container's** `data-block-pool-id`
  instead. No new attribute and no backend change — the container already carries
  `data-block-pool-id` (editable-as-whole since 2b; nested ones resolve via Plan
  3's path) and commits on the existing single-target channel. Add the container as
  a roving-tabindex stop preceding its first child (DOM pre-order), mirroring the
  section/heading split.
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
  contract (design Edge cases) at section scale. The guarantee that holds: bytes
  **outside the section and its boundary separators** stay byte-verbatim. (OQ2: the
  section becomes a `Rewrite` span with no `orig_idx`, so `compute_separator` falls
  back to the standard `"\n"` for the gaps adjacent to the range — a blank-line gap
  next to the section can normalize, same as any Tier-2 container rewrite.)
- **Boundary alignment:** the envelope must align to untransformed block
  boundaries; reject (read-only) if a range bisects a block.
- **Section with only generated content:** no envelope → no affordance (acceptable).
- **Re-sectionizing after edit:** adding/removing a heading re-shapes sections on
  the next render — handled by the normal round-trip; add a test if cheap.

## References
- Spec D7 (+ "writer already supports N→M"); `transforms/sectionize.rs`,
  `node_lookup.rs`, `apply_node_edit.rs`, `writers/incremental.rs`,
  `types/diagnostic.ts`, `q2-preview/blocks/Div.tsx`.
