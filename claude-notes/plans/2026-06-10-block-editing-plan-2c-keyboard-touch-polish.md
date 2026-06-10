# Block editing — Plan 2c: keyboard a11y, touch polish, Tier-2 tests

**Date:** 2026-06-10
**Branch:** feature/block-editing (worktree `.worktrees/block-editing`)
**Depends on:** Plans 2a, 2b, 3.
**Audit source:** 2026-06-10 post-2b audit — items that were either falsely
checked off in 2b (`useBlockEditHover` keyboard/ARIA) or left unchecked and not
yet filed anywhere (touch OS gesture suppression, Tier-2 WASM tests, P1 reflow
Playwright test).

## Overview

Three independent areas, roughly in priority order:

1. **Keyboard a11y** — roving-tabindex navigation + ARIA. Currently a keyboard
   user cannot reach editable blocks without a mouse hover first; Enter/Esc work
   but only after pointer interaction has set `hoveredRef`.

2. **Touch OS gesture suppression** — `touch-action: none`, `contextmenu`
   prevention, `-webkit-touch-callout: none`. The hold-to-activate mechanic works
   correctly, but on iOS the long-press triggers the OS copy/paste sheet
   simultaneously, which obscures the textarea.

3. **Tier-2 WASM round-trip tests** — `Table`, `Div`, `BlockQuote`, `BulletList`,
   `OrderedList`, `DefinitionList` edits verified end-to-end through the WASM
   pipeline. Para/Header/CodeBlock/RawBlock are already covered.

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

**`hoveredRef` unification.** Currently `hoveredRef` tracks the pointer-hovered
element. For keyboard nav, arrow keys call `el.focus()` AND set
`hoveredRef.current = el` so that the existing Enter/Space handler in `onKeyDown`
continues to work unchanged.

**`tabIndex={-1}` placement.** Each of the 16 leaf components that sets
`data-block-pool-id` must also set `tabIndex={-1}` on the same element (iff
`isEditable`). This is a one-liner alongside the existing attribute spread. The
host div already exists; `tabIndex={0}` is added to `hostProps` in
`useBlockEditHover`.

**ARIA.** Do not override semantic roles (`<p>`, `<h2>`, etc. are already
meaningful). Instead:
- The host div gets `role="region"` and `aria-label="Editable preview"` (via
  `hostProps`).
- Each editable block gets `aria-keyshortcuts="Enter"` — a standards-compliant
  hint that screen readers surface as "press Enter" alongside the element label.
  Unlike `aria-label`, `aria-keyshortcuts` does not replace the accessible name.
- A single visually-hidden hint element `<span id="q2-edit-hint">` inside the
  host reads "Use arrow keys to navigate blocks; press Enter or Space to edit"
  and is referenced by `aria-describedby="q2-edit-hint"` on the host.

### TDD work items

- [ ] **Tests first** (`useBlockEditHover.integration.test.tsx`):
  - Arrow keys move focus: `ArrowDown` focuses the next `[data-block-pool-id]`
    sibling in DOM order; `ArrowUp` focuses the previous.
  - Wrapping: `ArrowDown` on the last block focuses the first; `ArrowUp` on the
    first focuses the last.
  - Space key activates (separate test from Enter — Space is handled but currently
    untested).
  - `tabIndex={-1}` is present on editable block elements (one assertion per
    component type is sufficient — pick Para + Header).
  - `role="region"` + `aria-label` on the host element.
  - `aria-keyshortcuts="Enter"` on an editable block element.
  - Axe audit: `axe(container)` returns no violations for the mounted preview
    document.

- [ ] **Implementation** (`useBlockEditHover.tsx`):
  - Add `tabIndex: 0` and `role: 'region'` and `aria-label: 'Editable preview'`
    and `aria-describedby: 'q2-edit-hint'` to `hostProps`.
  - Render a `<span id="q2-edit-hint" style={{ ... /* visually hidden */ }}>` in
    the `stylesheet`/return value (or as a sibling node alongside `stylesheet`).
  - In `onKeyDown`, handle `ArrowDown` and `ArrowUp`:
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
  - Change Enter/Space handler to prefer `document.activeElement` over
    `hoveredRef.current` when the host is keyboard-focused (i.e. when
    `document.activeElement` has `data-block-pool-id`):
    ```ts
    const target = (document.activeElement?.closest('[data-block-pool-id]')
        ?? hoveredRef.current) as Element | null;
    if (target) { e.preventDefault(); activate(target); }
    ```

- [ ] **16 leaf components** — add `tabIndex={-1}` and `aria-keyshortcuts="Enter"`
  alongside the existing `data-block-pool-id` spread:
  ```tsx
  // existing:
  {...(isEditable ? { 'data-block-pool-id': poolId } : {})}
  // becomes:
  {...(isEditable ? {
      'data-block-pool-id': poolId,
      tabIndex: -1,
      'aria-keyshortcuts': 'Enter',
  } : {})}
  ```
  Files: `blocks/Para.tsx`, `blocks/Header.tsx`, `blocks/Div.tsx`,
  `blocks/BulletList.tsx`, `blocks/OrderedList.tsx`, `blocks/BlockQuote.tsx`,
  `blocks/DefinitionList.tsx`, `blocks/CodeBlock.tsx`, `blocks/RawBlock.tsx`,
  `blocks/Table.tsx`, `blocks/Figure.tsx`, `blocks/LineBlock.tsx`,
  `custom/Callout.tsx`, `custom/Theorem.tsx`, `custom/Proof.tsx`,
  `custom/FloatRefTarget.tsx`.

  Note: `dispatchers.tsx` does not add a wrapper element, so no change there.

## 2 — Touch OS gesture suppression

**Problem.** On iOS, a 500ms pointer hold triggers both our hold-activate path
and the OS long-press sheet (copy / lookup / share). The OS sheet overlaps the
textarea we activate.

**Fix — three lines:**

1. CSS in the injected stylesheet:
   ```css
   [data-block-pool-id] {
       -webkit-touch-callout: none;   /* suppress iOS callout on long-press */
       touch-action: none;            /* suppress scroll/zoom interference during hold */
   }
   ```

2. `onContextMenu` in `hostProps`:
   ```ts
   onContextMenu: (e: React.MouseEvent<HTMLElement>) => {
       if ((e as any).pointerType !== 'mouse') e.preventDefault();
   }
   ```
   Suppress the context-menu event for non-mouse pointers (long-press on touch)
   to prevent the OS copy/paste sheet. Mouse right-click is preserved.

### TDD work items

- [ ] **Stylesheet test** (`useBlockEditHover.integration.test.tsx`): assert that
  `touch-action` and `-webkit-touch-callout` are present in the injected stylesheet
  string. Simple string assertion on the rendered `<style>` content — no actual
  touch simulation needed.
- [ ] **`onContextMenu` test**: fire a `contextmenu` event with `pointerType !==
  'mouse'`; assert `preventDefault` was called. Fire with `pointerType === 'mouse'`;
  assert it was not called.
- [ ] **Implementation**: add the two CSS properties to the `<style>` string; add
  `onContextMenu` to `hostProps`. (Three-line change total.)

## 3 — Tier-2 WASM round-trip tests

**Plan 2b required** (unchecked): snapshot-based round-trips for `Table` +
each whole-container type. Outside bytes verbatim; edited block serialised but
not byte-asserted (wholesale rewrite accepted).

File: `hub-client/src/services/applyNodeEdit.wasm.test.ts` (add after the
RawBlock describe block at line 497).

- [ ] `Div` round-trip: fenced `:::` div between two paragraphs; edit the div's
  content; outer paras byte-verbatim.
- [ ] `BlockQuote` round-trip: `>` quote between paragraphs; edit; outer verbatim.
- [ ] `BulletList` round-trip: loose list between paragraphs; edit; outer verbatim.
  (Sibling items may be reformatted — no byte-identity assertion on siblings.)
- [ ] `OrderedList` round-trip: same shape as BulletList.
- [ ] `DefinitionList` round-trip: `.definition-list` div desugared; edit a body
  item; outer verbatim.
- [ ] `Table` round-trip: a simple markdown table between paragraphs; edit the
  whole table block; outer verbatim.

Each test follows the exact same structure as the existing CodeBlock test: render
through WASM → extract pool entry → build subtree → `apply_node_edit` → assert.

## 4 — P1 reflow Playwright test (deferred)

Plan 2b listed this as requiring a browser. It remains deferred: no automated
test currently verifies that editing a block does not shift its following sibling's
`getBoundingClientRect().top`. The sizing logic (given a `rect`, textarea matches
`width`/`height`) is already RTL-tested. Add a Playwright test here when the
Playwright suite is next run/expanded; it belongs alongside the existing
`q2-preview-inline-edit.spec.ts` tests.

- [ ] **Playwright** (`q2-preview-inline-edit.spec.ts`): activate edit on a heading
  followed by a paragraph; assert `paragraph.getBoundingClientRect().top` before
  and after activation are within ±1px (use `page.evaluate` to capture both
  measurements inside the iframe). Mark the test with `test.fixme` initially if
  the measurement is flakey.

## End-to-end verification

- [ ] `npm run preflight` (ts-packages/preview-renderer): integration tests green
  (including axe assertions).
- [ ] `npm run test:ci` (hub-client): WASM tests green.
- [ ] `npm run build:all` (hub-client): production bundle clean.
- [ ] Dev server: Tab into the preview iframe, navigate with arrow keys, activate
  with Enter; verify no layout shift. On iOS (or iOS Simulator): long-press to
  activate; verify OS copy sheet does not appear.
