# Block editing — Plan 2c: keyboard a11y, touch polish, Tier-2 tests

**Date:** 2026-06-10
**Branch:** feature/block-editing-improvements (worktree `.worktrees/block-editing`)
**Depends on:** Plans 2a, 2b, 3 (all done).
**Audit source:** 2026-06-10 post-2b audit — items that were either falsely
checked off in 2b (`useBlockEditHover` keyboard/ARIA) or left unchecked and not
yet filed anywhere (touch OS gesture suppression, Tier-2 WASM tests, P1 reflow
Playwright test). Section 0 (margin regression) discovered during review.

## Overview

**Implementation order:** 0 → 1 → 2 → 3 → 4 (the plan's priority order). Sections 0, 1,
and 2 all touch `useBlockEditHover.tsx`; working through them in sequence avoids
merge conflicts within that file. Section 3 (WASM tests) is independent of the
TypeScript work and can run between any sections, but 3 after 2 keeps a clean
commit history. Section 4 (Playwright) requires Section 0 to be done first (the
structural margin guarantee it tests).

Four areas, in priority order:

0. **Edit-surface box fix (regression from Plan 3)** — Plan 3 moved textarea
   substitution to the `Block`/`CustomBlock` dispatchers, which replaced the
   original element with a bare `<textarea>`. The element's CSS box (margins,
   padding, border) was no longer reproduced, so neighbours shifted on
   edit-start and decorations (an h2's Bootstrap `border-bottom` rule)
   disappeared. **As-built fix:** replace every editable block with a synthetic
   `<div>` whose inline style reproduces the element's full computed box
   (margin + padding + per-side border), captured via `getComputedStyle` at
   activation — the *measure-and-set* approach, applied uniformly to all block
   types. (An initial `EditContentContext` "wrap the original element" hybrid
   was tried and discarded — see § 0.)

1. **Keyboard a11y** — roving-tabindex navigation + ARIA. Currently a keyboard
   user cannot reach editable blocks without a mouse hover first; Enter/Esc work
   but only after pointer interaction has set `hoveredRef`.

2. **Touch OS gesture suppression** — `touch-action: none`, `contextmenu`
   prevention, `-webkit-touch-callout: none`. The hold-to-activate mechanic works
   correctly, but on iOS the long-press triggers the OS copy/paste sheet
   simultaneously, which obscures the textarea.

3. **Tier-2 WASM round-trip tests** — `Table`, `Div`, `BlockQuote`, `BulletList`,
   `OrderedList`, `DefinitionList`, `Figure` edits verified end-to-end through
   the WASM pipeline. Para/Header/CodeBlock/RawBlock are already covered.
   Plan 3's Rust tests (`node_edit_tests.rs`) cover nested descent; these WASM
   tests are top-level whole-block edits only.

## 0 — Edit-surface box fix (measure-and-set wrapper)

### Root cause

Plan 3 moved textarea substitution into the `Block`/`CustomBlock` dispatchers,
which replaced the editable element with a bare `<textarea>`. The element's CSS
box was not reproduced, so:

- **Spacing crunched.** A heading's margins/padding/border (Bootstrap `h2` has
  `padding-bottom: 0.5rem` + a `border-bottom`) vanished, pulling the following
  blocks up ~9.5px; the gap between a paragraph and a following list collapsed.
- **Decorations disappeared.** The visible rule under an `<h2>` (its
  `border-bottom`) was gone while editing.

### Approach considered and discarded: `EditContentContext` hybrid

The first attempt kept the *original element* in the DOM and injected the
textarea inside it via a React context (`EditContentContext`), so CSS was
inherited automatically; the four types whose root cannot legally contain a
`<textarea>` (`<ul>`/`<ol>`/`<dl>`/`<table>`) used a synthetic `<div>` with
captured margins instead. In practice the synthetic-`<div>` (measure-and-set)
types looked *better* than the element-wrapping ones, and the hybrid added a
dead `editOverride` branch to ~12 leaf components. **We discarded the hybrid**
and use measure-and-set for every type; `EditContentContext.tsx` and all the
`editOverride` lines were removed.

### As-built fix: measure-and-set, everywhere

On activation, `useBlockEditHover.activate()` captures the element's full
computed box into `editTarget.boxStyle` — a `Record<string,string>` of margin +
padding + per-side border longhands (incl. style & colour) — plus
`contentHeight` (content-area height for the textarea). The dispatcher's single
edit path, `renderMeasuredEdit()`, replaces the block with:

```tsx
<AttributionWrap node={node} as="div">
    <div style={{ ...boxStyle, boxSizing: 'content-box' }}>{textarea}</div>
</AttributionWrap>
```

`box-sizing: content-box` + `contentHeight` makes the wrapper's border-box
height equal the original element's height → zero reflow; replicating the full
per-side border keeps the h2 rule visible while editing.

**List left-inset strip.** `<ul>`/`<ol>`/`<dl>` carry a large `padding-left`
(the bullet/number gutter). Replicating it would indent the editing textarea
away from column 0 where the source markdown begins, so for
`LEFT_INSET_STRIPPED_TYPES` the wrapper zeroes `padding-left` / `margin-left` /
`border-left-width` (the vertical box is untouched).

### Focus restoration after commit

When a commit completes (or the user cancels with Esc), focus returns to the
edited block. Implemented in `entry.tsx` by wrapping the raw `useState` setter:
the last-edited pool id is remembered and, after the textarea unmounts,
`setTimeout(..., 0)` re-focuses `[data-block-pool-id="<id>"]` (deferred so the
synchronous focus move doesn't re-trigger the textarea's `onBlur` → double
commit). A racing new edit cancels the restore.

### Re-entrancy guards

Because `data-block-pool-id` stays in the DOM and the host's delegated handlers
fire during editing, `useBlockEditHover` bails early while an edit is active:
`onKeyDown`, `onPointerDown`, and `onPointerMove` each `return` when
`ctx?.editTarget != null`, and `activate()` no-ops when the clicked block is
already the edit target.

### Type changes

`editTarget` (`PreviewContext.tsx`) carries `{ poolId, contentHeight, boxStyle }`.
The earlier `rect` and the four discrete margin fields were removed as dead once
`boxStyle` subsumed them.

### TDD work items

- [x] **Structural tests** (`useEditableBlock.integration.test.tsx`): editing a
  `Para` replaces the `<p>` with a synthetic `<div>` that reproduces the box
  (asserts marginBottom, paddingBottom, the visible border-bottom, and that a
  non-list type's `padding-left` is preserved); editing a `BulletList` produces
  the same wrapper with the left inset stripped to `0px` and no `<ul>` in the
  DOM. (These replaced the original EditContentContext "the `<p>` stays in the
  DOM" assertions, which described the discarded approach.)
- [x] **Strong layout E2E** (`q2-preview-inline-edit.spec.ts`): `measureLayout`
  snapshots the viewport-top of every block + document scrollHeight (with a
  `fonts.ready` + double-rAF settle); `expectLayoutStable` asserts no other
  block moves and the document height is unchanged on activation. Scenarios:
  heading (with an explicit "h2 `border-bottom` rule persists" assertion),
  paragraph-before-list, and list-before-paragraph (with a "list textarea is not
  indented past the text column" assertion).
- [x] **Implementation**:
  - `useBlockEditHover.tsx`: `boxStyle` capture in `activate()`; the four
    re-entrancy guards
  - `dispatchers.tsx`: single `renderMeasuredEdit()` edit path for `Block` and
    `CustomBlock`; `LEFT_INSET_STRIPPED_TYPES` for the list gutter
  - `PreviewContext.tsx`: `editTarget = { poolId, contentHeight, boxStyle }`
  - `entry.tsx`: deferred focus restoration
  - Removed `EditContentContext.tsx` and the `editOverride` branch from all 12
    leaf/custom components (Para, Header, Div, BlockQuote, CodeBlock, Figure,
    LineBlock, RawBlock, Callout, Theorem, Proof, FloatRefTarget); also restored
    a missing `tabIndex: -1` on `CodeBlock`.

## 1 — Keyboard a11y

### Design

**Roving tabindex.** The `PreviewDocument` host div becomes the single Tab stop
(`tabIndex={0}` via `hostProps`). All editable blocks have `tabIndex={-1}` so
they are programmatically focusable but skipped by Tab. Arrow keys in
`onKeyDown` advance focus through the `[data-block-pool-id]` elements in DOM
pre-order; Enter/Space activates the currently focused one; Esc dismisses.

**Visual indicator.** The `:focus-visible` CSS already covers the outline when
an element with `tabIndex={-1}` receives programmatic `.focus()`. No additional
box-shadow management needed for keyboard — the CSS rule handles it.

**`hoveredRef` unification.** Arrow keys call both `el.focus()` AND
`hoveredRef.current = el`. This means the existing Enter/Space handler
(`if ((e.key === 'Enter' || e.key === ' ') && hoveredRef.current)`) continues
to work unchanged — `hoveredRef` is the single source of truth for both mouse and
keyboard navigation. No `document.activeElement` check is needed or correct: if
a block has keyboard focus while the mouse hovers a *different* block, checking
`document.activeElement` would activate the focused block unexpectedly.

**`tabIndex={-1}` placement.** Each of the 16 leaf components that sets
`data-block-pool-id` must also set `tabIndex={-1}` on the same element (iff
`isEditable`). This is a one-liner alongside the existing attribute spread. The
host div gets `tabIndex={0}` via `hostProps` in `useBlockEditHover`.

**ARIA scope note.** The keyboard design is for sighted keyboard users. Full
assistive-technology (screen reader) support for document editing is a larger
design problem — there is no standard ARIA pattern that cleanly covers
roving-tabindex navigation over semantic block elements (`<p>`, `<h2>`, etc.)
while preserving their reading semantics. Do NOT add `aria-keyshortcuts` or
`role="button"` to individual block elements; these either have poor AT support
or override the meaningful paragraph/heading roles. V1 claims keyboard
accessibility, not screen reader editing support.

**ARIA.** Do not override semantic roles (`<p>`, `<h2>`, etc. are already
meaningful). Instead:
- The host div gets `role="region"` and `aria-label="Editable preview"` (via
  `hostProps`). Note: `PreviewDocument` has a third rendering path — minimal mode
  with attribution disabled — that returns a Fragment with no host element; in
  that path `hostProps` is never spread, so these ARIA attributes are absent.
  This is intentional: that path is a passive read-only render, not an editing
  surface. See § Notes.
- A single visually-hidden hint element `<span id="q2-edit-hint">` inside the
  host reads "Use arrow keys to navigate blocks; press Enter or Space to edit"
  and is referenced by `aria-describedby="q2-edit-hint"` on the host. This fires
  once when the host first receives focus — not repeatedly on each block — so it
  is not intrusive. Note: `"q2-edit-hint"` is a hardcoded ID; multiple preview
  instances on the same page would collide. Single-instance use is assumed.

### TDD work items

- [x] **Tests first** (`useBlockEditHover.integration.test.tsx`):
  - Arrow keys move focus: `ArrowDown` focuses the next `[data-block-pool-id]`
    sibling in DOM order and sets `hoveredRef.current`; `ArrowUp` focuses the
    previous.
  - Wrapping: `ArrowDown` on the last block focuses the first; `ArrowUp` on the
    first focuses the last.
  - Space key activates (separate test from Enter — Space is handled but untested).
  - `tabIndex={-1}` is present on editable block elements (Para + Header sufficient).
  - `role="region"` + `aria-label` on the host element.
  - Axe audit: `axe(container)` returns no violations.

- [x] **Implementation** (`useBlockEditHover.tsx`):
  - Add `tabIndex: 0`, `role: 'region'`, `aria-label: 'Editable preview'`,
    `aria-describedby: 'q2-edit-hint'` to `hostProps`.
  - Render `<span id="q2-edit-hint" style={{ /* visually hidden */ }}>` alongside
    `stylesheet`.
  - In `onKeyDown`, handle `ArrowDown`/`ArrowUp`:
    ```ts
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        const host = e.currentTarget;
        const blocks = Array.from(
            host.querySelectorAll<HTMLElement>('[data-block-pool-id]'),
        );
        if (!blocks.length) return;
        const current = document.activeElement;
        const idx = blocks.indexOf(current as HTMLElement);
        const next = e.key === 'ArrowDown'
            ? blocks[(idx + 1) % blocks.length]
            : blocks[(idx - 1 + blocks.length) % blocks.length];
        next.focus();
        hoveredRef.current = next;
    }
    ```
  - Enter/Space handler is **unchanged** — `hoveredRef.current` (set by both
    pointer and arrow-nav) is the correct and sufficient source of truth.

- [x] **16 leaf components** — add `tabIndex={-1}` alongside the existing
  `data-block-pool-id` spread:
  ```tsx
  {...(isEditable ? {
      'data-block-pool-id': poolId,
      tabIndex: -1,
  } : {})}
  ```
  Files: `blocks/Para.tsx`, `blocks/Header.tsx`, `blocks/Div.tsx`,
  `blocks/BulletList.tsx`, `blocks/OrderedList.tsx`, `blocks/BlockQuote.tsx`,
  `blocks/DefinitionList.tsx`, `blocks/CodeBlock.tsx`, `blocks/RawBlock.tsx`,
  `blocks/Table.tsx`, `blocks/Figure.tsx`, `blocks/LineBlock.tsx`,
  `custom/Callout.tsx`, `custom/Theorem.tsx`, `custom/Proof.tsx`,
  `custom/FloatRefTarget.tsx`.

  Note on `LineBlock`: unreachable in the pampa parser (no tree-sitter grammar
  support for the `| line` syntax). The component code is consistent and harmless;
  no tests required for LineBlock specifically.

  Note: `dispatchers.tsx` does not add a wrapper element, so no change there.

## 2 — Touch OS gesture suppression

**Problem.** On iOS, a 500ms pointer hold triggers both our hold-activate path
and the OS long-press sheet (copy / lookup / share). The OS sheet overlaps the
textarea we activate. On Android, a long-press fires a `contextmenu` event.

**Fix — three lines:**

1. CSS in the injected stylesheet:
   ```css
   [data-block-pool-id] {
       -webkit-touch-callout: none;   /* suppress iOS callout on long-press */
       touch-action: pan-y;           /* allow vertical scroll; suppress pinch-zoom
                                        and horizontal pan during hold */
   }
   ```
   `-webkit-touch-callout: none` suppresses the iOS native callout. It has no
   Android equivalent; `onContextMenu` handles Android.

   **`touch-action: pan-y` not `none`:** `touch-action: none` would prevent the
   browser from scrolling the document when a touch gesture starts on a block
   element. Since blocks fill the page width, this makes the document unscrollable
   by touch. `pan-y` allows vertical panning (normal document scroll) while
   suppressing pinch-zoom and horizontal gestures during the hold window.
   The `MOVE_THRESHOLD_PX` guard in `onPointerMove` already cancels a hold if
   the finger moves, so a user who intends to scroll (vertical movement > 8px)
   will cancel the hold naturally — `pan-y` doesn't break hold-to-activate.

2. Track last pointer type via a `useRef` (set in `onPointerDown`, already called
   for all pointers) and read it in `onContextMenu`:
   ```ts
   const lastPointerTypeRef = useRef<string>('mouse');
   // in onPointerDown: lastPointerTypeRef.current = e.pointerType;
   // in hostProps:
   onContextMenu: (e: React.MouseEvent<HTMLElement>) => {
       if (lastPointerTypeRef.current !== 'mouse') e.preventDefault();
   }
   ```
   Suppress context-menu for touch long-press; preserve mouse right-click.
   **Note:** `contextmenu` fires as `MouseEvent` with no `pointerType` property —
   reading `(e as any).pointerType` always returns `undefined`, suppressing
   right-click too. The ref is the correct approach.

   **Ordering:** set `lastPointerTypeRef.current = e.pointerType` as the **first**
   line of `onPointerDown`, before the Section 0 editing guard. The guard returns
   early when editing is active, so if the ref update comes after it, a touch
   during an active edit leaves the ref stale. If editing then commits and the user
   immediately right-clicks, the ref would still read `'touch'` and suppress the
   right-click.

### TDD work items

- [x] **Stylesheet test** (`useBlockEditHover.integration.test.tsx`): assert
  `touch-action: pan-y` and `-webkit-touch-callout: none` are present in the
  injected `<style>` content. (Asserts `pan-y`, the value the body specifies,
  not the `none` shorthand in this line's original wording.)
- [x] **`onContextMenu` test**: dispatch `pointerdown` with `pointerType: 'touch'`
  to prime the ref, then dispatch `contextmenu` — assert `preventDefault` called.
  Dispatch `pointerdown` with `pointerType: 'mouse'`, then `contextmenu` — assert
  `preventDefault` NOT called.
- [x] **Implementation**: add CSS properties to `<style>` string; add
  `lastPointerTypeRef`; set it in `onPointerDown`; add `onContextMenu` to
  `hostProps`.

## 3 — Tier-2 WASM round-trip tests

**Scope.** Plan 3's Rust tests (`node_edit_tests.rs`, 44 tests) already cover
nested-descent correctness. These WASM tests exercise the complete top-level
pipeline (render → pool → edit → apply) for container types not yet covered.
Outside bytes verbatim; edited block serialised but not byte-asserted (wholesale
rewrite accepted — see Plan 3 investigation).

File: `hub-client/src/services/applyNodeEdit.wasm.test.ts` (add after the
RawBlock describe block).

- [x] `Div` round-trip: fenced `:::` div between two paragraphs; edit the div's
  content; outer paras byte-verbatim.
- [x] `BlockQuote` round-trip: `>` quote between paragraphs; edit; outer verbatim.
- [x] `BulletList` round-trip: loose list between paragraphs; edit; outer verbatim.
  (Sibling items may be reformatted — no byte-identity assertion on siblings.)
- [x] `OrderedList` round-trip: same shape as BulletList.
- [x] `DefinitionList` round-trip: a definition list between two paragraphs; edit
  the whole container via text channel; outer paragraphs byte-verbatim. This is a
  Tier-2 whole-block replacement, not a Plan 3 nested edit. QMD syntax for the
  fixture (confirmed — pampa renders this as `[Para, DefinitionList, Para]`):
  ```
  ::: {.definition-list}
  * term 1
    - definition body
  :::
  ```
  Note: `postprocess.rs` always fully rewrites `DefinitionList` (it's a desugared
  form of `Div(.definition-list)`), so the byte-identity assertion on the edited
  block is deliberately omitted.
- [x] `Table` round-trip: a simple GFM table between paragraphs; edit the whole
  table block; outer verbatim.
- [x] `Figure` round-trip: a cross-referenced figure between two paragraphs; edit
  the whole figure; outer verbatim. Fixture must use `{#fig-foo}` to produce a
  first-class `Figure` AST node:
  ```
  ![A caption.](img.png){#fig-foo}
  ```
  Add an exploratory note in the test describing what happens when the `{#fig-foo}`
  ID is absent: render the same fixture without it, check whether the AST node is
  still `Figure` and whether it is editable (has `data-block-pool-id`). We don't
  care if the id-less case doesn't work; the test just records the observed behavior
  so future readers know it was investigated.

Each test follows the same structure as the existing CodeBlock test: render
through WASM → extract pool entry → build subtree → `apply_node_edit` → assert.

## 4 — P1 reflow Playwright test

The measure-and-set wrapper (Section 0) reproduces the element's computed box, so
the zero-reflow guarantee must be confirmed in a real layout engine. The original
single-paragraph test grew into the **stronger layout suite** documented in
Section 0's TDD work items (`measureLayout` / `expectLayoutStable` over every
block + the document height, plus the h2-rule-persistence and list-not-indented
assertions). The simplest of these:

- [x] **Playwright** (`q2-preview-inline-edit.spec.ts`): activate edit on a
  heading followed by a paragraph; assert `paragraph.getBoundingClientRect().top`
  before and after activation are within ±1px (via `page.evaluate` inside the
  iframe). Shipped as a real test (not `test.fixme`) — verified stable 3/3 with
  `--repeat-each=3 --retries=0`. The stronger crunch/rule/indent scenarios were
  added in the Section 0 follow-up and run green across `--repeat-each=2`.

## Notes

**Host mode / editing affordances.** `PreviewDocument` has three rendering paths:
1. Standard mode — `<div id="quarto-content">` with full chrome; `hostProps` and
   `blockEdit.stylesheet` applied.
2. Minimal mode with attribution enabled — anonymous `<div>`; `hostProps` and
   `blockEdit.stylesheet` applied.
3. Minimal mode with attribution disabled — bare Fragment, no host element.
   `hostProps` is never spread; `blockEdit.stylesheet` is never rendered. There
   are no edit affordances in this path. This is intentional: this path is a
   passive read-only render (designed to be "byte-identical to today's" for
   comparison purposes), not an editing surface.

**LineBlock.** The `LineBlock` component has `data-block-pool-id` and would be
editable if it existed in rendered output. However, the pampa parser has no
tree-sitter grammar support for the `| line` block syntax, so `LineBlock` nodes
are unreachable in practice. The code is kept consistent; no tests are needed
for this type specifically.

**`Plain` block — intentionally not editable.** `Plain.tsx` renders `<>{renderChildren(args)}</>` with no wrapper element and no `data-block-pool-id`. This is correct: `Plain` nodes appear inside list items and table cells where direct text-channel editing is not meaningful. `Plain` is absent from all editable-component lists in this plan intentionally.

**Custom block root elements.** All four CustomBlock types render `<div>` as their outermost element: `Callout`, `Theorem`, `Proof` → `<div className={...}>`, `FloatRefTarget` → `<div id={...}>` (or `<figure>` for `fig`). With the as-built measure-and-set approach this no longer matters for editing — every editable block, custom or not, is replaced by the synthetic measure-and-set `<div>`, so there is no "wrappable vs. non-wrappable" distinction anymore.

**`aria-describedby` ID.** `"q2-edit-hint"` is a hardcoded global DOM ID.
Multiple preview instances on the same page would produce duplicate IDs. This
is a known limitation under the single-instance assumption; acceptable for v1.

## End-to-end verification

- [x] `npm run preflight` (ts-packages/preview-renderer): integration tests green
  (including axe assertions and new structural margin tests). (`preflight` script
  does not exist; ran the equivalent `test:integration` → 231 passed.)
- [x] `npm run test:ci` (hub-client): WASM tests green. (575 + 66 + 105 passed.)
- [x] `npm run build:all` (hub-client): production bundle clean. (exit 0.)
- [x] Dev server: edit a paragraph — verify the following paragraph does not shift.
  Edit a heading — verify layout is stable.
  (Confirmed live in the dev server during the measure-and-set follow-up — the
  user verified paragraph/heading/list spacing holds and the h2 rule persists
  ("works very well"). Also covered by the §4 stronger layout E2E suite. The
  hands-on keyboard arrow-nav pass specifically was not separately exercised.)
- [x] **Manual — iOS** (device or Simulator): long-press a paragraph to activate;
  verify OS copy/lookup sheet does not appear. Scroll the document by touch through
  a block; verify the page scrolls normally (`pan-y`). Activate a block; verify
  the textarea appears and is editable. (Confirmed by the user.)
- [x] **Manual — Android** (device or emulator): long-press a paragraph; verify no
  context menu appears. Scroll through blocks; verify normal scroll. Activate and
  edit a block. (Confirmed by the user.)
