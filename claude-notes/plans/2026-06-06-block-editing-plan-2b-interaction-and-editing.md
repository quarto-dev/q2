# Block editing — Plan 2b: interaction model + editing (built-in + render-component)

**Date:** 2026-06-06 (revised 2026-06-08: built on Plan 2a's dual-node substrate;
absorbed the former Plan 5 editability + Plan 6 render-component work;
revised 2026-06-08b: two-channel API, discriminated payload, editTarget rect, test environments, Pass-2 exact deletions)
**Branch:** feature/block-editing (worktree `.worktrees/block-editing`)
**Spec:** `claude-notes/designs/2026-06-06-block-editing-design.md`
**Phase:** 2b. Frontend (+ one Rust cleanup: remove `lookup_block` Pass-2).
**Depends on:** Plan 2a (dual-node dispatch + structural gate); Plan 1 (editor).

## Overview

Turn every editable block into its own affordance (no pencil) and make both
**built-in editing** (the markdown textarea) and **render-component editing**
(custom TSX that manipulates the AST) work in q2-preview, on Plan 2a's
`PreviewContext.sourceIndex` substrate. Hover / press / focus **outlines** the deepest editable
target; activating opens the editor.

**Win:** edit any top-level block type — para/heading/code/raw/table and
whole-container div/quote/lists — by mouse, touch, or keyboard; and the three
render-component demos (drag/comment/kanban) edit through the source tree.

## Design prerequisites (settle first)

### P1 — Editor sizing: no-reflow height match
On edit-start, render the `<textarea>` at the measured box of the element it
replaces (`box-sizing: border-box`, matched width/height/margins) → **zero
document reflow**. *Acceptance:* the following sibling's `getBoundingClientRect().top`
is unchanged (±1px). (Overflow scrolls internally; accepted.)

**Sizing mechanism:** `useBlockEditHover` measures the activated element's
`DOMRect` synchronously in the event handler (before state update) and stores it
in `editTarget.rect`. `useEditableBlock` reads `editTarget.rect` to size the textarea.

**Test environments:** The reflow criterion (sibling top ±1px) requires real
layout — it is a **Playwright** test in `q2-preview-spa/`. The sizing *logic*
(given a provided `rect`, textarea gets correct `width`/`height`/`margin`) is an
**RTL** test with a mocked `getBoundingClientRect`.

### P2 — Editor font: body-sized monospace
Monospace at ≈ **0.9 × computed body font-size** (`getComputedStyle`), uniformly
for all block types, so a heading edits as body-sized monospace, not huge text.
*Test:* RTL — assert the computed `fontFamily` is monospace and `fontSize` ≈ 0.9×
body in the rendered textarea.

### P3 — Auto-fit font: considered, NOT implemented
Scaling the font so `lineHeight × lineCount == box height` was rejected (logical
vs wrapped lines diverge; width-aware is circular). Recorded only.

## Interaction model (no pencil)

The block element *is* the affordance. One delegated handler on the
`PreviewDocument` root finds the deepest editable block via
`closest('[data-block-pool-id]')` (the `useAttributionHover` delegation shape,
`attribution.tsx:189`), outlines it (layout-safe `outline`/`box-shadow`, no
wrapper — D4), and activates the editor:

- **Mouse:** hover outlines; click activates.
- **Touch (Pointer Events):** one progressive press — `pointerdown` outlines
  (reveal); hold past `HOLD_MS` activates; early release / move-beyond-threshold
  cancels. `setPointerCapture`; suppress OS gestures (`touch-action: none`,
  `preventDefault` on `contextmenu`, `-webkit-touch-callout: none`).
- **Keyboard (roving tabindex):** the edit layer is a single Tab stop; arrows
  move the active region in DOM pre-order; Enter/Space activates; Esc exits;
  `:focus-visible` reuses the outline; ARIA role + name per region.

No overlay to position, so there is no scroll-tracking problem. (Plan 4's
heading/section split adds a glyph-rect target and may need a drawn highlight;
out of scope here.)

## Editability (from Plan 2a — no gate machinery here)

A node is editable iff `editable(node)` (Plan 2a's structural gate:
`ctx.resolveSource(node)?.reachabilityClass === 'TopLevel' && editableType`). 2b just
**consumes** it — spread `data-block-pool-id` on a block's own root iff editable;
the affordance keys off that. **Editable types** (all `TopLevel`): `Para`,
`Header`, `CodeBlock`, `RawBlock`, `Table` (Tier-2), and **as a whole**
`Div`(non-section), `BlockQuote`, `BulletList`, `OrderedList`, `DefinitionList`
(Tier-2). Container *contents* are gated off until Plan 3. **LineBlock excluded**
(parses as `Para`). **Custom-rendered nodes** (`Callout`/`Theorem`/`Proof`/
`FloatRefTarget`) are editable-as-whole too: their `sourceNode` is a plain `Div`
(writer-covered), so the `custom/` components must also spread
`data-block-pool-id` and participate — this is the former Plan 5 "editability
wiring," now just part of 2b.

**Activation model change:** The per-block `onClick` handlers on Para and Header
(Plan 1's seed mechanism) are **removed**. `useBlockEditHover` becomes the sole
activation path — it calls `setEditTarget({ poolId, rect })` on click/press/Enter.
Editable components keep their `editTarget.poolId === myPoolId` check to decide
when to render the textarea; only the trigger changes.

## Two edit modalities, two channels

Both channels are first-class and of equivalent power; they differ in *representation*.

- **Built-in editing → text channel.** The textarea shows the block's **source
  markdown**, obtained by slicing `content` over `sourceNode`'s range (Plan 1's
  `sliceBytes`); on commit the component calls `ctx.commitTextEdit(destinationSourceInfoJson, newText)`;
  the parent runs `parseQmdContentSync(newText)` then `apply_node_edit`.
  (The iframe can't run the writer, so text is the right representation for the textarea.)

- **Render-component editing → subtree channel.** A component obtains its
  `sourceNode` via `ctx.resolveSource(args.node)` — the same `PreviewContext`
  call built-in components use — modifies a `structuredClone` of `resolved.sourceNode`
  (the untransformed block; unexpanded, shortcodes/refs intact), then calls
  `ctx.commitSubtreeEdit(destinationSourceInfoJson, modifiedBlock)`. The parent
  passes the serialized block **straight to `apply_node_edit`**, skipping the text
  parse. `destinationSourceInfoJson` is `JSON.stringify(resolved.sourceEntry)` in
  both channels.

**`PreviewContext` gains two functions** (replacing the old `commitEdit`):

```typescript
commitTextEdit:    (destinationSourceInfoJson: string, newText: string) => void;
commitSubtreeEdit: (destinationSourceInfoJson: string, modifiedBlock: BlockNode) => void;
```

**`editTarget` changes type** to carry the measured rect for P1 sizing:

```typescript
editTarget?: { poolId: string | number; rect: DOMRect } | null;
```

**`PreviewNodeEditPayload` becomes a discriminated union** on the wire
(`types/diagnostic.ts`):

```typescript
type PreviewNodeEditPayload =
  | { __isPreviewNodeEdit: true; channel: 'text';
      destinationSourceInfoJson: string; newText: string }
  | { __isPreviewNodeEdit: true; channel: 'subtree';
      destinationSourceInfoJson: string; modifiedSubtreeJson: string };
```

The parent routes on `channel`: `'text'` → parse then splice; `'subtree'` → splice
directly. Each editable node commits **itself, targeted by its own `sourceNode`
SourceInfo** — no copy-on-write bubble in preview (the Rust reconcile rebuilds to
root). This is the q2-preview routing; q2-debug/slides keep the
bubble→whole-AST→`incrementalWriteQmd` path (Plan 2a, Format routing).

**Demos migration note:** The existing render-component demos (`drag`, `comment`,
`kanban`) previously committed via `setLocalAst` which routed through the
incremental write pipeline using the *transformed* node directly. They are broken
since the pipeline changed. Plan 2b migrates them to use `sourceNode` (the
untransformed counterpart from `resolveSource`) + `commitSubtreeEdit`.

## Render-component / built-in boundary (the one design decision)

1. **Wrapping:** the framework never wraps a block (D4). A component author *may*
   wrap (e.g. `comment`'s `position:relative` div) but **owns** the theme-CSS /
   hit-test consequences; the affordance keys off `data-block-pool-id` on the
   block's **own root**, which authors preserve by rendering the block through the
   framework dispatcher (`<B>`/`renderChildren`).
2. **Composition:** an overridden block that renders the underlying block through
   the framework still gets the built-in affordance (so a paragraph can be both
   commented on *and* text-edited); the component's UI layers around it.
3. **Opt-out:** a component may mark its subtree **not-built-in-editable** (skip
   `data-block-pool-id`) when it deliberately hides source structure (e.g.
   `comment` renders with comment-spans stripped, while the real source still
   contains them — acceptable for v1; opt-out is the escape hatch).

## Backend: require Pass-1 exact match; remove `lookup_block` Pass-2

`lookup_block` Pass-2 (the `preimage_in` covering fallback) is unused on every
live path; for a gate-emitted `TopLevel` `Original` commit, Pass-1 must succeed,
and a covering hit could only mean a mis-gated sub-range that would silently
replace a container.

- [ ] Scope the commit guard to **`TopLevel`** edits: require a Pass-1 exact
  match, **no-op + warn** otherwise. (Plan 3's `Descendable` edits resolve by
  path, not this guard.)
- [ ] **Remove `lookup_block` Pass-2.** Concrete deletions:
  - `crates/pampa/src/node_lookup.rs` lines 55–72 (the Pass-2 block); the
    function then falls through to `None` after the Pass-1 check.
  - `crates/pampa/tests/integration/node_edit_tests.rs`: delete test
    `lookup_finds_block_via_generated_preimage_fallback` (lines 175–198).
  - `claude-notes/plans/2026-06-04-target-incremental-writes.md`: update three
    spots — data-flow diagram comment (line 98), editability gate prose
    (lines 144–146), and Phase 2 checklist part (b) (lines 210–219) — to
    document that property #2 is retired and `Generated` nodes now return `None`.

## TDD work items (tests first)

### Tests
- [ ] **Editor sizing P1 — Playwright** (`q2-preview-spa/`): activate an edit on
  a heading and a paragraph; assert the following sibling's
  `getBoundingClientRect().top` is unchanged (±1px).
- [ ] **Editor sizing P1 — RTL logic**: given a mocked `rect` stored in
  `editTarget`, `useEditableBlock` renders a textarea whose `width`, `height`, and
  `margin` match the rect. RTL with mocked `getBoundingClientRect`.
- [ ] **Editor sizing P2 — RTL**: textarea `fontFamily` is monospace and computed
  `fontSize` ≈ 0.9× body for a heading and a paragraph.
- [ ] `useBlockEditHover` mouse: deepest editable block via `closest`; outline
  clears on edit-start; `editTarget.rect` is populated from the element's
  `getBoundingClientRect()` at activation.
- [ ] Touch progressive-press (Pointer Events): `pointerdown` outlines; hold past
  `HOLD_MS` activates; early up / move cancels; `pointerType` branch.
- [ ] Keyboard roving tabindex: one Tab stop; arrows move in DOM pre-order;
  Enter activates; Esc exits; only active region `tabindex=0`.
- [ ] Affordance honors Plan 2a's gate: generated (`t:4`), included (`d≠0`), and
  container-nested blocks show **no** affordance; a paragraph at top level or only
  inside a section does; a callout (`Div.callout-*`) shows the affordance.
- [ ] Pass-1 guard: a synthetic `TopLevel` edit that only *covers* no-ops + warns.
- [ ] Tier-1 round-trip (CodeBlock/RawBlock; re-confirm Para/Heading): edit lands,
  surrounding blocks byte-verbatim.
- [ ] Tier-2 round-trip (snapshotted): `Table` + each whole-container — outside
  byte-verbatim, edited block matches a snapshot; **no byte-identity** assertion.
- [ ] **Text channel routing:** a `channel: 'text'` payload triggers
  `parseQmdContentSync` then `apply_node_edit`; blocks outside the target stay
  byte-verbatim.
- [ ] **Subtree channel routing:** a `channel: 'subtree'` payload routes straight
  to `apply_node_edit` (no parse); blocks outside the target stay byte-verbatim.
- [ ] **Render-component demos (RTL + edit-survival):** `drag` commits Div attrs;
  `comment` appends a span to the block's `sourceNode`; `kanban` reorders the
  `Div`'s untransformed children. Each uses `commitSubtreeEdit` with
  `resolved.sourceNode`; surrounding content intact.
- [ ] *(Plan 4 boundary)* a section shows **no** affordance yet
  (`data-section-range` not emitted until Plan 4).

### Implementation
- [ ] `q2-preview/useEditableBlock.tsx` — shared editor (P1 sizing from
  `editTarget.rect`, P2 font); activation comes from the hover/press/keyboard
  layer (no `onClick`-only path).
- [ ] `q2-preview/useBlockEditHover.tsx` — delegated pointer handler; outline;
  Pointer-Events progressive press; roving-tabindex keyboard; ARIA; measures
  `DOMRect` at activation and writes `setEditTarget({ poolId, rect })`.
- [ ] `PreviewDocument.tsx` — spread the hook's `hostProps` on the root host
  (`~:263` main / `~:238` minimal); render `stylesheet` node from hook.
- [ ] **`PreviewContext`** — replace `commitEdit(poolId, newText)` with
  `commitTextEdit(destinationSourceInfoJson, newText)` and
  `commitSubtreeEdit(destinationSourceInfoJson, modifiedBlock: BlockNode)`; change
  `editTarget` type to `{ poolId: string | number; rect: DOMRect } | null`.
- [ ] **`types/diagnostic.ts`** — replace `PreviewNodeEditPayload` with the
  `channel`-discriminated union (see "Two edit modalities" above).
- [ ] **`entry.tsx`** — update `commitTextEdit`/`commitSubtreeEdit` implementations
  to build the appropriate payload variant; parent routes on `channel`.
- [ ] **Para.tsx, Header.tsx** — remove `onClick`/`setEditTarget(poolId)`;
  add `data-block-pool-id={poolId}` (iff editable); update commit call to
  `commitTextEdit(JSON.stringify(resolved.sourceEntry), text)`; read sizing from
  `editTarget.rect`.
- [ ] Spread `data-block-pool-id` (iff `editable`) on all other editable-type
  components, **including the `custom/` ones** (Callout/Theorem/Proof/
  FloatRefTarget).
- [ ] Fix `drag`/`comment`/`kanban` in
  `~/docs/demo-playground/gordon/render-components2/` onto the
  `resolveSource` + `commitSubtreeEdit` model (replacing `setLocalAst` calls with
  subtree-channel commits targeting `resolved.sourceNode`).
- [ ] Rust: Pass-1 guard (TopLevel-scoped) + remove `lookup_block` Pass-2 (see
  "Backend" section for exact lines).

## End-to-end verification
- [ ] `npm run build:wasm` + dev server: outline-on-hover + edit for
  para/heading/code/raw/table/top-level div/quote/list and a callout (whole);
  touch progressive-press; keyboard Tab→arrow→Enter→Esc; a block inside a `:::`
  div / `Figure` body / `Table` cell shows no affordance. Then exercise the three
  demos end-to-end and confirm the `.qmd` keeps shortcodes/refs inside the edited
  region (i.e. `sourceNode`, not the expansion). Record steps + output.
- [ ] `npm run build:all`.

## Known limitations (this phase)
- LineBlock not editable; container **contents** (Plan 3), **sections** (Plan 4)
  not yet; selection/link-vs-click accepted; generated-container regions (e.g.
  appendix) gated off.

## Risks / watch-items
- **Attribution + edit-hover coexistence** — when attribution is enabled
  (currently inert in q2-preview), two delegated pointer handlers + two highlights
  share `#quarto-content`; interaction undesigned; human-in-the-loop will observe.
- **Touch long-press** co-opts the OS selection/context-menu gesture.
- **Submit is not a no-op** (Tier-2); only cancel is guaranteed.

## References
- Spec D1–D10, "Interaction model", "Two edit modalities", "Format routing";
  Plan 2a; `framework/`, `q2-preview/{PreviewDocument,dispatchers,entry,
  PreviewContext}.tsx`, `q2-preview/blocks/*`, `q2-preview/custom/*`;
  `~/docs/demo-playground/gordon/render-components2/{drag,comment,kanban}.tsx`.
