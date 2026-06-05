# Block editing — Plan 4: section editing (range replace)

**Date:** 2026-06-06
**Branch:** feature/block-editing (worktree `.worktrees/block-editing`)
**Spec:** `claude-notes/designs/2026-06-06-block-editing-design.md`
**Phase:** 4 of 4. Rust (`pampa` + WASM) + frontend.
**Depends on:** Plans 1–3.

## Overview

Make whole **sections** editable. A section is a sectionize-generated `Div`
(`.section`) wrapping a heading + body blocks; its `source_info` is
`Generated{by:sectionize, from:[]}` → **not sliceable**, and it spans **multiple**
untransformed top-level blocks. So (D7): the **frontend** computes the section's
source **envelope** `[min start, max end]` over its `Original` descendants and
sends a **range** edit; the **backend** adds a range lookup + range splice. The
reconcile/writer core is **unchanged** — it already replaces a contiguous N
blocks with M blocks while preserving bytes outside the span (verified in the
spec).

**Win:** edit a whole section (heading + body) at once; nested sections handled
by the envelope.

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
  `PreviewNodeEditPayload` gains an optional `range: [number, number]`
  (discriminates a section edit from a single-target edit).
- [ ] `q2-preview/blocks/Div.tsx` — when `.section` and an envelope exists,
  carry `data-section-range`; the editor uses the range payload; section pencils
  go live (remove the Plan 2 suppression).
- [ ] `q2-preview/utils/sectionEnvelope.ts` (new).
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
- **Section with only generated content:** no envelope → no pencil (acceptable).
- **Re-sectionizing after edit:** adding/removing a heading re-shapes sections on
  the next render — handled by the normal round-trip; add a test if cheap.

## References
- Spec D7 (+ "writer already supports N→M"); `transforms/sectionize.rs`,
  `node_lookup.rs`, `apply_node_edit.rs`, `writers/incremental.rs`,
  `types/diagnostic.ts`, `q2-preview/blocks/Div.tsx`.
