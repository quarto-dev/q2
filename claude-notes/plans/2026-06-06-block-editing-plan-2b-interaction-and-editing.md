# Block editing — Plan 2b: interaction model + editing (built-in + render-component)

**Date:** 2026-06-06 (revised 2026-06-08: built on Plan 2a's dual-node substrate;
absorbed the former Plan 5 editability + Plan 6 render-component work;
revised 2026-06-08b: two-channel API, discriminated payload, editTarget rect, test environments, Pass-2 exact deletions;
revised 2026-06-09: usePreviewEdit hook for render-component authors; boundary section rewrite; backend guard clarification; isEditTarget + setEditTarget type fixes; hostProps note;
**post-execution accuracy note 2026-06-10:** `editTarget` gained `contentHeight` field (see P1); `useEditableBlock.tsx` was created then deleted in Plan 3, which moved all textarea logic to `Block`/`CustomBlock` dispatchers; `reachabilityClass` gate widened to `!== 'Opaque'` in Plan 3; LineBlock and Figure got `data-block-pool-id` in Plan 3.)
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
in `editTarget.rect` (plus a `contentHeight` field — `rect.height` minus padding
and border — so the textarea fills the content area exactly without reflow even
when the element has padding/border, e.g. Bootstrap `h2`). `useEditableBlock`
(later: the `Block` dispatcher — see Plan 3) reads `editTarget.rect` and
`editTarget.contentHeight` to size the textarea.

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
(Tier-2). Container *contents* are gated off until Plan 3. **Custom-rendered nodes**
(`Callout`/`Theorem`/`Proof`/`FloatRefTarget`) are editable-as-whole too: their
`sourceNode` is a plain `Div` (writer-covered), so the `custom/` components must
also spread `data-block-pool-id` and participate — this is the former Plan 5
"editability wiring," now just part of 2b.

**(Plan 3 post-execution update)** The gate was widened from `=== 'TopLevel'` to
`!== 'Opaque'` in Plan 3, unlocking `Descendable` blocks (nested content). `Figure`
and `LineBlock` also received `data-block-pool-id` in Plan 3 (Figure was omitted from
the editable-type list here; LineBlock was incorrectly listed as excluded — note that
LineBlock is unreachable in the pampa parser currently and would be editable if it
existed). All textarea substitution was centralized in `Block`/`CustomBlock`
dispatchers; `useEditableBlock.tsx` was created for Plan 2b then deleted in Plan 3.

**Activation model change:** The per-block `onClick` handlers on Para and Header
(Plan 1's seed mechanism) are **removed**. `useBlockEditHover` becomes the sole
activation path — it calls `setEditTarget({ poolId, rect, contentHeight })` on click/press/Enter.
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

**`editTarget` changes type** to carry the measured rect and content-area height for P1 sizing:

```typescript
editTarget?: { poolId: string | number; rect: DOMRect; contentHeight: number } | null;
```

(`contentHeight` = `rect.height` minus computed padding and border; the textarea uses this so it fills the content area without reflow even on elements with padding or border.)

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
untransformed counterpart from `resolveSource`) + `commitSubtreeEdit`, accessed
via the new `usePreviewEdit()` hook (see below).

**Demo file convention:** The canonical copies live at
`~/docs/demo-playground/gordon/render-components2/` (outside the repo — updated
during dev sessions). Playwright fixtures for the RTL + edit-survival tests are
copies placed inside the repo under `q2-preview-spa/e2e/helpers/` or a
`crates/quarto/tests/smoke-all/` fixture directory.

## Render-component API (`usePreviewEdit`)

Render-component authors access `resolveSource` and `commitSubtreeEdit` through a
**hook on the renderer global surface** — no need to know about `PreviewContext`
or React context directly.

`window.__Q2_PREVIEW_RENDERER__.usePreviewEdit` returns:

```typescript
{
  resolveSource:    (node: BlockNode) => ResolvedSource | null;
  commitSubtreeEdit:(destinationSourceInfoJson: string, modifiedBlock: BlockNode) => void;
  commitTextEdit:   (destinationSourceInfoJson: string, newText: string) => void;
}
```

The hook is a thin wrapper over `useContext(PreviewContext)`. When the context is
absent (q2-debug, q2-slides — where `PreviewContext` is never provided) it returns
nullish functions, so components that call `commitSubtreeEdit?.()` degrade
silently. q2-debug components continue to use `args.setLocalAst` on their own path;
the q2-preview path uses `usePreviewEdit`.

A migrated `drag.tsx` looks like:

```tsx
const { renderChildren, usePreviewEdit } = window.__Q2_PREVIEW_RENDERER__;
export const Div = (args) => {
  const edit = usePreviewEdit();
  // ...on drag end:
  const resolved = edit.resolveSource(args.node);
  if (resolved) {
    const modified = structuredClone(resolved.sourceNode);
    modified.c[0][2] = [['x', newX + ''], ['y', newY + '']];
    edit.commitSubtreeEdit(JSON.stringify(resolved.sourceEntry), modified);
  }
};
```

`NodeArgs` (`framework/types.ts`) remains unchanged. `PreviewContext` is not
exposed on the global — `usePreviewEdit` is the public surface.

## `data-block-pool-id` placement and composition

The affordance attribute lives on the block's **own root element**, never on a
framework-added wrapper — because the framework never wraps (D4).
`useBlockEditHover`'s `closest('[data-block-pool-id]')` finds it there.

Custom components that render the block through `<B>` or `renderChildren`
preserve the attribute automatically. A component that wraps the block output
(e.g. `comment`'s `position:relative` div) gets both its overlay UI **and** the
built-in text-edit affordance on the inner element — no extra work needed.

A component that replaces the rendered block entirely (no `<B>` delegation,
fully custom HTML) emits no `data-block-pool-id` and gets no built-in affordance
— a natural consequence of not delegating, not a distinct mechanism. If such a
component wants the affordance it spreads `data-block-pool-id={poolId}` on its
own root.

## Backend: remove `lookup_block` Pass-2; no-op on stale-AST miss

`lookup_block` Pass-2 (the `preimage_in` covering fallback) is already unreachable
on every live path: `decode_compact_source_info` in `apply_node_edit` rejects any
non-`t=0` SourceInfo before `lookup_block` is called, so a `Generated` target
never arrives. Pass-2 is dead code. For an `Original` (`t=0`) commit that
passes the frontend gate, Pass-1 must succeed under the value-equality invariant;
a covering hit could only mean a mis-gated sub-range that would silently replace
a container.

The behavior change for `lookup_block → None` is: instead of surfacing
`DestinationNotFound` as an error, **no-op + warn** (log the miss, return the
original `content` unchanged). This degrades gracefully on a stale-AST race
(render N+1 fires before the edit from render N lands) rather than showing the
user an error. `DestinationNotFound` is removed from the error enum entirely.
(Plan 3's `Descendable` edits will resolve by path, not this guard.)

- [x] **Remove `lookup_block` Pass-2.** Concrete deletions:
  - `crates/pampa/src/node_lookup.rs` lines 55–72 (the Pass-2 block); the
    function then falls through to `None` after the Pass-1 check.
  - `crates/pampa/tests/integration/node_edit_tests.rs`: delete test
    `lookup_finds_block_via_generated_preimage_fallback` (lines 178–200; find
    by function name — line numbers may drift).
  - `claude-notes/plans/2026-06-04-target-incremental-writes.md`: update three
    spots — data-flow diagram comment (~line 98), editability gate prose
    (~lines 144–147), and Phase 2 checklist part (b) (~lines 211–218) — to
    document that property #2 is retired and `Generated` nodes now return `None`.
    Find by content rather than exact line number.
- [x] **Change `apply_node_edit` step 3** from `.ok_or(DestinationNotFound)?`
  to: `eprintln!` the miss (pampa uses `eprintln!` directly; no `log`/`tracing`
  crate in scope) + `return Ok(content.to_string())`.
- [x] **Remove `ApplyNodeEditError::DestinationNotFound`** from the error enum
  and its `Display` arm (it has no other callers).

## TDD work items (tests first)

### Tests
- [x] **Editor sizing P1 — RTL logic**: given a mocked `rect` stored in
  `editTarget`, `useEditableBlock` renders a textarea whose `width`, `height`, and
  `margin` match the rect.
  *Test: `ts-packages/preview-renderer/src/q2-preview/useEditableBlock.integration.test.tsx`*
- [x] **Editor sizing P2 — RTL**: textarea `fontFamily` is monospace and computed
  `fontSize` ≈ 0.9× body.
  *Same test file as P1.*
- [x] `useBlockEditHover` mouse: mouse activation fires `setEditTarget` with numeric
  poolId + DOMRect; outline clears on activation.
  *Test: `ts-packages/preview-renderer/src/q2-preview/useBlockEditHover.integration.test.tsx`*
- [x] Touch progressive-press: hold past HOLD_MS activates; early up / move cancels.
  *Same test file.*
- [x] Keyboard: Enter activates hovered element; Esc calls `setEditTarget(null)`.
  *Same test file. Note: roving-tabindex arrow navigation and Space key are covered
  in Plan 2c.*
- [x] Affordance honors Plan 2a's gate: generated (resolveSource returns null) → no
  `data-block-pool-id`; TopLevel Para → has it; section Div → no attribute; non-section
  Div with TopLevel source → has it.
  *Tests: `ts-packages/preview-renderer/src/q2-preview/q2-preview.integration.test.tsx`*
- [x] Stale-AST miss guard: a `lookup_block`-returns-None scenario no-ops + emits
  `eprintln!`; the original `content` is returned unchanged (not an error).
  *Test: `node_edit_tests::stale_ast_miss_noops_and_returns_original_content`.*
- [x] Tier-1 round-trip (CodeBlock/RawBlock): edit lands, surrounding blocks
  byte-verbatim. (Para/Header covered by pre-existing tests.)
  *Tests: `hub-client/src/services/applyNodeEdit.wasm.test.ts`* (run with `npm run test:wasm`)
- [x] **Text channel routing:** a `channel: 'text'` payload triggers
  `parseQmdContentSync` then `apply_node_edit`.
  *Tests: `q2-preview-spa/src/channelRouting.integration.test.tsx`*
- [x] **Subtree channel routing:** a `channel: 'subtree'` payload routes straight
  to `apply_node_edit` (no parse); `modifiedSubtreeJson` passed as 4th arg.
  *Same test file.*
- [x] **Render-component demos (RTL + edit-survival):** drag/comment/kanban
  migrated to `usePreviewEdit()` + `commitSubtreeEdit`; Playwright E2E tests pass.
- [x] *(Plan 4 boundary)* section Div with TopLevel source shows **no** `data-block-pool-id`
  (`section` class check fires before `resolveSource`).
  *Test: `ts-packages/preview-renderer/src/q2-preview/q2-preview.integration.test.tsx`*

**→ Plan 2c** carries the remaining items: Tier-2 WASM round-trips (Table +
containers), roving-tabindex + ARIA, touch OS gesture suppression
(`touch-action`/`contextmenu`/`-webkit-touch-callout`), Space key test, and the
P1 reflow Playwright test. See
`claude-notes/plans/2026-06-10-block-editing-plan-2c-keyboard-touch-polish.md`.

### Implementation
- [x] `q2-preview/useEditableBlock.tsx` — shared editor (P1 sizing from
  `editTarget.rect`, P2 font); activation comes from the hover/press/keyboard
  layer (no `onClick`-only path).
- [x] `q2-preview/useBlockEditHover.tsx` — delegated pointer handler; outline;
  Pointer-Events progressive press; keyboard (Enter/Esc) activation; measures
  `DOMRect` at activation and writes `setEditTarget({ poolId, rect, contentHeight })`.
  *Roving-tabindex arrow navigation, ARIA, and touch OS gesture suppression are
  in Plan 2c.*
- [x] `PreviewDocument.tsx` — spread the hook's `hostProps` on the root host
  after `attr.hostProps` (lines ~263 main / ~238 minimal); render `stylesheet`
  node from hook. The two `hostProps` sets are disjoint today (`useAttributionHover`
  uses `onMouseOver`/`onMouseOut`; `useBlockEditHover` uses pointer events) so a
  second spread is correct. If a future handler introduces a key overlap, a
  compose helper will be needed at that point.
- [x] **`PreviewContext`** — replace `commitEdit(poolId, newText)` with
  `commitTextEdit(destinationSourceInfoJson, newText)` and
  `commitSubtreeEdit(destinationSourceInfoJson, modifiedBlock: BlockNode)`; change
  `editTarget` type to `{ poolId: string | number; rect: DOMRect; contentHeight: number } | null`;
  change `setEditTarget` type to
  `(target: { poolId: string | number; rect: DOMRect; contentHeight: number } | null) => void`.
- [x] **`types/diagnostic.ts`** — replace `PreviewNodeEditPayload` with the
  `channel`-discriminated union (see "Two edit modalities" above).
- [x] **`entry.tsx`** — update `commitTextEdit`/`commitSubtreeEdit` implementations
  to build the appropriate payload variant; parent routes on `channel`; add
  `usePreviewEdit` to `window.__Q2_PREVIEW_RENDERER__` surface.
- [x] **`usePreviewEdit` hook** — thin wrapper: `useContext(PreviewContext)` →
  return `{ resolveSource, commitSubtreeEdit, commitTextEdit }` (nullish when
  context absent). Lives in `q2-preview/usePreviewEdit.ts`; exported from
  `q2-preview/index.ts`.
- [x] **Para.tsx, Header.tsx** — remove `onClick`/`setEditTarget(poolId)`;
  add `data-block-pool-id={poolId}` (iff editable); change `isEditTarget` check
  from `ctx!.editTarget === poolId` to `ctx!.editTarget?.poolId === poolId`;
  update commit call to `commitTextEdit(JSON.stringify(resolved.sourceEntry), text)`;
  read sizing from `editTarget.rect`. (Shared logic extracted into `useEditableBlock`.)
- [x] Spread `data-block-pool-id` (iff `editable`) on all other editable-type
  components, **including the `custom/` ones** (Callout/Theorem/Proof/
  FloatRefTarget). Also: CodeBlock, RawBlock, BlockQuote, BulletList, OrderedList,
  DefinitionList, Div (non-section), Table.
- [x] Fix `drag`/`comment`/`kanban` onto the `usePreviewEdit()` +
  `commitSubtreeEdit` model. Done; Playwright E2E specs pass for all three
  demos.
- [x] Rust: remove `lookup_block` Pass-2; change `apply_node_edit` miss to
  no-op + warn; remove `DestinationNotFound` (see "Backend" section for exact lines).

## End-to-end verification
- [x] `npm run build:all` green (repeated throughout implementation).
- [x] Automated tests: 3929 Rust / 556+66+210 TypeScript tests green.
- [ ] Dev server keyboard Tab→arrow→Enter→Esc: deferred to Plan 2c (roving
  tabindex not yet implemented).

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
