# Preview rich-text editor: open with drag-selection preserved

**Strand:** bd-abo9m23f
**Builds on:** bd-q9lyghv2 (caret-at-click), whose design comments live in
`ts-packages/preview-renderer/src/q2-preview/richtext/caretFromClick.ts` and
`RichTextEditor.tsx`.

## Overview

In `q2 preview --allow-edit` (and `format: q2-preview` in hub-client), clicking
a block opens the rich-text editor with the caret placed at the click point
(bd-q9lyghv2). But when the user *drags* a selection inside a single block, the
`pointerup` at the end of the drag activates the editor with only a **caret at
the release point** — the selection the user just made is discarded. Since the
rich-text toolbar (bold, italic, link, …) operates on the selection, this
forces a re-select inside the freshly opened editor.

**Goal:** when the DOM selection at activation time is non-collapsed and fully
contained in the block being activated, open the rich-text editor with an
equivalent editor selection (anchor and head mapped from the DOM selection,
direction preserved).

**Also in scope** (decided 2026-07-07, was open question 1): when the
selection at a mouse activation is non-collapsed but NOT contained in the
activated block (cross-block drag), **suppress activation entirely** — today
the release block opens with a caret and the DOM swap destroys the user's
selection, which breaks plain copy gestures. Mouse-open paths only; keyboard
and touch activation are untouched. (The user has separate follow-ups planned
around rich-text activation mechanisms; this stands on its own.)

**Out of scope** (current behavior stays):
- The plain-text `EditTextarea` surface → caret behavior as today (possible
  follow-up: map to `selectionStart`/`selectionEnd`, but it needs a
  coords→textarea-offset mapping that doesn't exist; not worth it now).
- Keyboard/touch activation → end-of-block, as today.

## Reproduction (2026-07-07)

Fixture: a `.qmd` with a few paragraphs/lists (any multi-word paragraph works).

```
cargo run --bin q2 -- preview repro.qmd --allow-edit --no-browser
# → http://127.0.0.1:61115/?page=repro.qmd
```

Scripted repro against the live preview (chrome-devtools MCP; CDP's `drag` is
an HTML5-DnD gesture and never builds a text selection, so the drag end-state
was synthesized — a real `Range` over "quick brown fox jumps over the lazy"
set as the window selection, then a `pointerup` dispatched at the coordinates
of the end of "lazy", exactly the state a completed mouse drag leaves):

- **Before `pointerup`:** `getSelection().toString()` = `"quick brown fox
  jumps over the lazy"`, fully inside the paragraph (`data-block-pool-id="5"`).
- **After `pointerup`:** rich-text editor open and focused; selection is a
  collapsed `Caret` with context `…fox jumps over the lazy‸ dog, and then…` —
  i.e. caret at the release point, drag selection lost. (Screenshot inspected:
  caret visible between "lazy" and "dog", no highlight.)

Feasibility probes (same session):
1. The native selection **is intact and readable at `pointerup` time** in the
   capture phase (verified with capture-phase listeners).
2. Collapsed clone-ranges at the selection endpoints give usable viewport
   rects (`range.cloneRange(); collapse(); getClientRects()[0]`).
3. **Geometric stability across the swap:** after the editor mounted, the
   pre-swap endpoint coordinates resolved (via `caretRangeFromPoint`) to
   exactly `|quick` and `lazy|` **inside the ProseMirror editor** — the same
   invariant caret-at-click relies on (same text, same measured box, same
   theme CSS).

## Design

Extend the existing coordinate bridge from one point to an anchor/head pair.
Same architecture as bd-q9lyghv2 — no new mechanism, just a richer payload:

```
useBlockEditHover.activate()          RichTextEditor mount effect
  capture selection endpoint coords →  pendingClickCoordsRef  →  posAtCoords ×2
  (viewport space, direction-aware)      (read-once + clear)      TextSelection(anchor, head)
```

### 1. Payload type (PreviewContext.tsx, PreviewRoot.tsx)

Rename/retype `pendingClickCoordsRef` payload from `{x, y} | null` to:

```ts
type PendingOpenSelection =
  | { kind: 'caret'; head: { x: number; y: number } }
  | { kind: 'range'; anchor: { x: number; y: number }; head: { x: number; y: number } }
  | null;
```

Read-once-and-clear semantics unchanged (self-heal remounts still fall back to
end-of-block; after a reflow *both* endpoints are stale, so the same reasoning
applies a fortiori).

### 2. Capture (useBlockEditHover.tsx, inside `activate()`)

`activate()` is the single open chokepoint and already resolves `outerBlock`,
so containment is checked there (not in `onPointerUp`, which doesn't know the
resolved block). When `opts.clickCoords` is present (mouse open):

1. `const sel = outerBlock.ownerDocument.defaultView.getSelection()` —
   `ownerDocument`-relative, not bare `window`, since the preview runs inside
   an iframe.
2. If `sel` is non-collapsed, `rangeCount > 0`, and **both** the range's
   `startContainer` and `endContainer` are inside `outerBlock` → compute
   viewport coords of the **anchor** and **focus** points (selection API, not
   range start/end — preserves drag direction for backward drags) via
   collapsed clone-ranges + `getClientRects()[0]`, and stash
   `{ kind: 'range', anchor, head }`.
3. If `sel` is non-collapsed but NOT contained in `outerBlock` (cross-block
   drag) → **abort the activation**: return from `activate()` before any side
   effect (before `cancelPendingLand`/`captureGeometry`/draft seeding), leaving
   the user's selection intact for copying. A plain click can't false-trigger
   this: the browser collapses/clears the selection on `mousedown`, so by
   `pointerup` a non-drag click always sees a collapsed selection.
4. Otherwise (collapsed / no rects / `rangeCount === 0`) → stash
   `{ kind: 'caret', head: opts.clickCoords }` — exactly today's behavior.

Note the degenerate case: a drag contained in block A but *released* over
block B classifies as cross-block relative to B (the activated block) →
suppressed, selection preserved. Consistent with decision 2 below (we don't
hunt for the selection's home block).

The capture is a pure helper (own module, e.g. `dragSelectionCapture.ts`) so
it unit-tests without a real layout — a three-way classification:
`classifyOpenSelection(outerBlock, clickCoords): PendingOpenSelection |
'suppress'` (where `'suppress'` makes `activate()` return early; only for
mouse opens — keyboard/touch never reach this code since they carry no
`clickCoords`).

Containment against `outerBlock` means the check is automatically mode-aware
(leaf in unlock-nesting mode, outer block in locked mode) — it tests the
element actually being activated.

### 3. Replay (caretFromClick.ts + RichTextEditor.tsx mount effect)

New sibling to `placeCaretFromClick`:

```ts
placeSelectionFromDrag(editor, anchor, head): boolean
  // posAtCoords both endpoints; if BOTH hit:
  //   TextSelection.create(doc, anchorHit.pos, headHit.pos)  — direction preserved
  //   dispatch + focus; return true
  // else return false
```

Note: tiptap's `setTextSelection({from, to})` normalizes/clamps and does not
document direction preservation — use ProseMirror directly
(`view.dispatch(state.tr.setSelection(TextSelection.create(...)))` +
`editor.commands.focus()`), matching the file's existing "single source of
truth" stance.

Mount-effect fallback chain (inside the existing rAF):

1. `kind: 'range'` → `placeSelectionFromDrag`; on miss ↓
2. head coords → `placeCaretFromClick` (caret at release point, today's
   behavior); on miss ↓
3. `editor.commands.focus('end')` (historical default).

### 4. Edge cases

| Case | Behavior |
| --- | --- |
| Backward drag (right→left) | anchor/focus coords keep direction; head = release point, so shift-arrow keeps extending naturally |
| Multi-line drag within block | endpoints on different lines — two independent `posAtCoords`, fine |
| Selection exactly = whole block (triple-click) | contained → range replay; if the browser range leaks outside the block element, containment fails → suppressed (selection preserved). Note triple-click on an *unopened* block never reaches this path anyway: click #1's `pointerup` already opened the editor, clicks #2/#3 hit the active-region guard and ProseMirror handles word/para select natively |
| Cross-block drag (selection spans blocks) | activation suppressed, selection preserved for copying (decision 1, 2026-07-07) |
| Drag contained in block A, released over block B | classifies cross-block relative to B → suppressed, selection preserved |
| Drag across styled inlines (bold/em) | coords land on glyphs; `posAtCoords` doesn't care about inline structure |
| Double-click word-select | first click's `pointerup` already opened the editor, so the second click hits the active-region guard and ProseMirror handles word-select natively — not this path |
| Drag ends outside any block | `findEditTarget` returns null → no activation (unchanged) |
| Click-switch (editor A open, drag in block B) | flows through the same `activate()` chokepoint → works |
| Self-heal remount | ref already cleared → end-of-block (unchanged, per read-once) |
| `richTextAvailable` false → textarea surface | textarea ignores the ref (today it already ignores coords); unchanged |

### Decisions (2026-07-07, user)

1. **Cross-block drags: suppress activation** (included in this work, § 2
   above). Today the release block activates and the DOM swap destroys the
   selection — bad for plain copy gestures. The user has separate follow-ups
   planned around rich-text activation mechanisms; this improvement stands on
   its own. *(Within-block copy-drags are incidentally fixed too: the
   selection survives into the editor.)*
2. **Drag releasing outside every block: leave as-is** (nothing activates).
   The dominant use cases are short "drag, then bold / link" selections;
   resolving the target from the selection's ancestor is corner-casey,
   brittle, and too early in the feature's life.

## Work items (TDD ordering)

### Phase 0 — tests first

- [x] Unit: `dragSelectionCapture.test.ts` — mocked selection/ranges:
  contained → `{kind:'range'}` with direction-aware anchor/head; cross-block →
  `'suppress'`; collapsed → caret; `rangeCount === 0` → caret; empty
  `getClientRects()` → caret; invalid endpoint offset → caret.
- [x] Unit: extend `caretFromClick.test.ts` — `placeSelectionFromDrag` against
  a REAL EditorState (richTextSchema): both-hit → selection dispatched with
  direction preserved + focus, `true`; either-miss → nothing moved, `false`;
  degenerate same-pos → `false`.
- [x] Integration: `RichTextEditor.caret.integration.test.tsx` — mount with a
  `range` payload → `placeSelectionFromDrag` called with both coord pairs;
  range-miss → caret fallback at head; ref read-and-cleared; `caret` payload
  keeps existing path (regression).
- [x] Integration: `useBlockEditHover.caret-coords.integration.test.tsx` —
  mouse `pointerup` with stubbed contained selection stashes a `range`
  payload; with cross-block selection `setEditTarget` is NOT called
  (suppression); keyboard activation with a live selection still opens
  (suppression is mouse-only); keyboard/touch stash nothing (regression).
- [x] Run new tests, verify they fail (2026-07-07: 11 fail for missing
  module/export + old ref name; pre-existing caret tests still pass).
  Note: integration tests run via `npm run test:integration`
  (`vitest.integration.config.ts`) — the default config excludes them.

### Phase 1 — implementation

- [x] `PreviewContext.tsx` / `PreviewRoot.tsx`: ref renamed
  `pendingClickCoordsRef` → `pendingOpenSelectionRef`, union payload
  (`PendingOpenSelection`), doc comments updated.
- [x] New `dragSelectionCapture.ts` pure helper (`classifyOpenSelection` +
  the `PendingOpenSelection` type; endpoint coords via collapsed clone-range
  rects, `ownerDocument`-relative selection lookup for iframe safety).
- [x] `useBlockEditHover.tsx` `activate()`: classify after the dedup guard and
  before any side effect; `'suppress'` returns early; stash payload at the
  single chokepoint.
- [x] `caretFromClick.ts`: `placeSelectionFromDrag` via
  `TextSelection.between(resolve(anchor), resolve(head))` (direction
  preserved, endpoints nudged to valid text positions; empty ⇒ false so the
  caret fallback owns collapsed cases).
- [x] `RichTextEditor.tsx` mount effect: fallback chain range → caret → end.
- [x] preview-renderer suites green (2026-07-07: 508 unit + 533 integration
  passed, 0 failed).
- [x] `npm run test:ci` (hub-client): unit + integration legs green; the
  `test:wasm` leg fails on the PRE-EXISTING `changelog.md renders
  successfully` test — the 2026-07-07 changelog entry's literal `~37MB`
  parses as an unclosed subscript (Q-2-17). Broken on main (commit
  `6cd4dd5a`), unrelated to this strand; an open PR already fixes it
  (bd-5px05kui filed and closed as duplicate).

### Phase 2 — end-to-end verification

- [x] Playwright e2e `q2-preview-spa/e2e/drag-selection-open.spec.ts` — real
  trusted mouse drags via `mouse.down/move/up` against the real `q2` binary
  (embedded SPA rebuilt first: `cargo xtask build-q2-preview-spa` +
  `cargo build -p quarto --bin q2`). Three tests, all passed 2026-07-07:
  forward drag (editor selection === selection recorded at `pointerup` in a
  capture-phase listener), backward drag (direction preserved), cross-block
  drag (no editor, selection intact). `npx playwright test
  drag-selection-open` → `3 passed (2.4s)`.
- [x] Manual (2026-07-07): `./target/debug/q2 preview repro.qmd --allow-edit`
  (fresh binary), drove the preview at `http://127.0.0.1:62037/` via
  chrome-devtools MCP. With the DOM selection "quick brown fox jumps over the
  lazy" in the first paragraph, `pointerup` opened the rich-text editor with
  `selectionType: "Range"`, `selectionText: "quick brown fox jumps over the
  lazy"`, selection anchored inside `.ProseMirror` (screenshot inspected —
  highlight visible in the open editor). Clicking the toolbar **B** on the
  carried selection produced exactly
  `<strong>quick brown fox jumps over the lazy</strong>` — the
  selection-driven-toolbar fluidity this strand is for.
- [x] Full `cargo xtask verify` (2026-07-07): Rust build + tests, ts-packages
  builds, hub-client `build:all` all green; hub-client tests green except the
  pre-existing changelog.md Q-2-17 failure noted above (fix in flight in an
  open PR, independent of this change).

## Outcome

Shipped as `4af08ef3` (feature) + `d116a917` (hub-client changelog entry) on
branch `braid/bd-abo9m23f-drag-selection-richtext`, 2026-07-07, rebased onto
main's `658026d0` (which fixed the pre-existing changelog Q-2-17 failure noted
above). Strand bd-abo9m23f closed.

## Notes

- `posAtCoords` endpoint bias: the PoC resolved `|quick` / `lazy|` exactly at
  word boundaries, so no bias correction is planned. If e2e shows a
  one-position drift at line wraps, clamp/bias there — not speculatively.
- Timing: selection is final by `pointerup` for drags (browsers build it
  during `pointermove`; verified readable in capture phase). The e2e test
  locks this in with trusted events.
- jsdom can't do geometry (`posAtCoords`/`getClientRects` are null/empty) —
  unit tests mock the view/ranges, geometry truth lives in the e2e test. Same
  split bd-q9lyghv2 used.
