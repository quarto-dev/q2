# Block editing — Plan 2: hover pencil + generalized block editor

**Date:** 2026-06-06
**Branch:** feature/block-editing (worktree `.worktrees/block-editing`)
**Spec:** `claude-notes/designs/2026-06-06-block-editing-design.md`
**Phase:** 2 of 4. Frontend only. No Rust changes.
**Depends on:** Plan 1 (the textarea edit mechanism + `content` plumbing).

## Overview

Add a **sticky** hover pencil (top-right of the deepest editable block under the
cursor) and generalize the Plan-1 textarea editor to **every source-backed
top-level block** (code, quote, bullet/ordered/definition lists, **tables**,
para, heading). Modeled on the existing
`useAttributionHover` (`attribution.tsx:189`) — delegated handler on the
`PreviewDocument` root, `closest()` for deepest block, floating overlay — but
**sticky** (D9), because the attribution badge clears on mouse-out and a corner
pencil would vanish before it can be clicked.

**Win:** pencils on every applicable block; click-edit any top-level block type.

## Scope

**In:** `useBlockEditHover` (sticky, scroll-anchored); a pencil button (rounded
rect, "not too obtrusive"); `data-block-pool-id` spread on block roots; a shared
`useEditableBlock` hook factored out of Plan 1's Para/Header; the
inside-container editability gate; click-to-edit retained on Para/Header
**alongside** the pencil.

**Out:** sections (pencil suppressed — Plan 4); blocks nested inside a
non-section container (gated off — Plan 3); generated/non-`Original` blocks (no
pencil, D3).

## Editability gate (precise)
Editable iff `Original (t:0)` **and** `file_id == 0` **and** **not inside a
non-section container**. Implemented via an `InsideContainerContext`: `Div`
without `.section`, `BulletList`/`OrderedList`/`DefinitionList`, and
`BlockQuote` push `true`; sections push nothing (transparent). This exactly
matches what the existing top-level `lookup_block` can commit; Plan 3 removes the
container restriction.

## Round-trip fidelity: Tier 1 vs Tier 2 (submit re-serializes)
Generalizing the editor beyond para/heading surfaces the two-tier round-trip
contract (see the design's "Submit is not a no-op"). Committing **always**
re-serializes the edited block through the writer; the writer may reformat **in
the area of the change**:

- **Tier 1 — verbatim-safe:** Para, Heading, CodeBlock. Source has no structural
  punctuation the writer normalizes; edits (and resubmits) come back faithfully.
- **Tier 2 — reformat-on-reserialize:** BlockQuote, BulletList/OrderedList,
  **DefinitionList**, and **Table**. The writer legitimately rewrites bullet
  chars (`*`/`+`→`-`), ordered-list markers + **renumbering**, blockquote `>`
  reflow, definition-list `:` layout, and — the extreme — **table column padding
  / alignment-row widths** (a table essentially never survives byte-identical).

The contract we test is therefore **not** global byte-identity. For every type:
blocks **outside** the edited one stay byte-verbatim; **inside** the edited
block, Tier-2 reformatting is **accepted** and **snapshotted** (visible in
review, not a failure). This is the same wholesale-vs-recurse behavior Plan 3
investigates for nested containers — here it applies to the top-level block.

## TDD work items (tests first)

### Tests
- [ ] `useBlockEditHover` unit/RTL: `closest('[data-block-pool-id]')` returns the
  **deepest** block; hovering a child then its parent updates the active block;
  **sticky** — active block persists after the cursor leaves it until a
  different editable block is hovered; clears on edit-start.
- [ ] **Editability-gate — pure predicate, all three conjuncts.** Unit-test the
  gate `(entry, insideContainer) → editable` against synthetic pool entries:
  editable **only** for `{t:0, d:0}` with `insideContainer = false`; **not**
  editable for `{t:0, d:1}` (included file), `{t:1|2|4, …}`
  (Substring/Concat/Generated), and `{t:0, d:0}` with `insideContainer = true`.
  The `t` and `d` conjuncts are exactly the holes Plan 1 leaves open — they
  **must** be verified here.
- [ ] **Editability-gate — integration (realistic shapes):** (a) a
  filter/shortcode-**generated** paragraph (`t:4`, `r:[0,0]`) shows **no**
  pencil; (b) an **included-file** block (`d≠0`, via a `{{< include >}}`
  fixture — cf. the INCLUDE_DOC case, D8) shows **no** pencil; (c) a block
  inside a non-section `Div`/list/`BlockQuote` shows **no** pencil; the same
  block at top level (or only inside a section) **does**.
- [ ] **Gate governs the retained click path, not just the pencil.** Clicking a
  *non-editable* Para/Header (generated, included, or container-nested) is
  **inert** — no textarea, no `setEditTarget`. Plan 2 keeps click-to-edit
  alongside the pencil via the shared `useEditableBlock`; both must read the same
  gate, or Plan 1's click hole survives past Plan 2.
- [ ] RTL: hovering a `CodeBlock`/`BlockQuote` shows the pencil; clicking it
  enters the textarea editor with sliced markdown.
- [ ] Pencil position tracks scroll (anchored in content, not stale `fixed`).
- [ ] *(Plan 4 boundary)* a **section** block itself shows **no** pencil yet.
  This passes trivially in Plans 2-3: `data-section-range` is **not** added to
  section `Div` roots until Plan 4, so the `closest()` selector never matches a
  section. Do **not** add `data-section-range` to `Div.tsx` as forward prep in
  Plan 2 — if you do, you must also explicitly suppress the pencil for those
  elements or you'll leak a section pencil before the backend is ready.
- [ ] **Tier-1 round-trip (verbatim-safe):** edit a top-level `CodeBlock` (and
  re-confirm Para/Heading); assert the edit lands and **surrounding blocks are
  byte-verbatim**. Do not assert anything stronger than the surrounding-verbatim
  guarantee.
- [ ] **Tier-2 round-trip (reformat accepted, snapshotted):** for each of
  `BlockQuote`, `BulletList`, `OrderedList`, `DefinitionList`, and `Table` —
  edit the block via the WASM path and assert (a) blocks **outside** stay
  byte-verbatim, and (b) the re-serialized edited block matches an **insta/Vitest
  snapshot** (pins bullet/marker/renumber/`>`/`:`/padding normalization so it's
  reviewable, not a hard failure). **Explicitly do NOT assert byte-identity**,
  especially for `Table` (guaranteed to re-pad).
- [ ] **Renumbering is observable:** editing one item of an `OrderedList` started
  at `1) 1) 1)` (or non-sequential) snapshots the writer's renumbered output —
  documents that "submit reformats" is real and intended, not a regression.

### Implementation
- [ ] `q2-preview/useBlockEditHover.ts(x)` (new) — delegated `onMouseOver` on the
  root host; `closest('[data-block-pool-id], [data-section-range]')`; sticky
  active-block state; returns `hostProps` + a positioned pencil overlay anchored
  to the active block within the scrolling content.
- [ ] `q2-preview/PreviewDocument.tsx` — spread the hook's `hostProps` on the
  same root host that carries `attr.hostProps` (`~:238,263`); render the pencil
  overlay sibling (alongside `attr.overlay`).
- [ ] Pencil button component — rounded-rect, pencil glyph, sized to click but
  unobtrusive; `onClick` sets `editTarget`.
- [ ] Factor Plan 1's Para/Header editor into a shared `useEditableBlock` /
  `<EditableBlock>` and adopt it in all block components: `blocks/Para.tsx`,
  `Header.tsx`, `Div.tsx`, `CodeBlock.tsx`, `BlockQuote.tsx`, list components,
  and `Table.tsx`. Each spreads `data-block-pool-id` on its root **iff editable**
  (Tier-2 types are still editable — they just reformat on commit).
- [ ] `InsideContainerContext` (new) pushed by container components; the gate
  reads it.

## End-to-end verification
- [ ] `npm run build:wasm` + dev server: confirm pencils appear on hover for
  para/heading/code/quote/top-level list/**table**; clicking edits via textarea;
  edit a **table** and confirm it reformats (re-padded) but renders correctly and
  surrounding content is untouched; a block inside a fenced `:::` div shows **no**
  pencil (gated). Record steps + output.
- [ ] `npm run build:all`.

## Risks / watch-items
- **Pencil occlusion:** the corner button must not block links/content; small +
  only on the active block.
- **Scroll/resize:** reposition the sticky pencil on scroll/resize of the iframe
  document.
- **`cloneElement` vs spread:** confirmed blocks return single roots; prefer an
  explicit `data-block-pool-id` spread on each root over `cloneElement`.

## References
- Spec D3, D4, D9; `framework/attribution.tsx` (`useAttributionHover`),
  `q2-preview/PreviewDocument.tsx`, `q2-preview/dispatchers.tsx`.
