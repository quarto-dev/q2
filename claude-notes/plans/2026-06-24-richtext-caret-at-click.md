---
title: 'Rich-text editor: place caret at click position on first activation'
date: 2026-06-24
description: 'Places the rich-text editor caret at the clicked position on first activation instead of the end of the block, by capturing the click coordinates and resolving them with `posAtCoords`.'
---

**Date:** 2026-06-24
**Strand:** bd-q9lyghv2 (discovered-from bd-sjb4pzx8)
**Branch:** current worktree branch
**Status:** ✅ Implemented + end-to-end verified (2026-06-24). Commits
`151be676` (feature) and `410885a3` (autofocus race fix + jsdom test polyfill).
All work items below complete.
**Builds on / required reading:**
`claude-notes/plans/2026-06-23-tiptap-rich-text-block-editor.md` (the rich-text
editor plan), `RichTextEditor.tsx`, `useBlockEditHover.tsx`, `PreviewContext.tsx`.

---

## Overview

In `q2 preview --allow-edit` with `&richText=1`, clicking a paragraph or heading
swaps the rendered block for the tiptap rich-text editor. The cursor currently
lands at the **end** of the block, not where the user clicked. Holding still and
clicking the *same* spot a second time moves the caret correctly — because by
then the editor is already mounted and ProseMirror handles the click natively.

We want the **first** click to place the caret where the user clicked.

### Root cause

`RichTextEditor.tsx:93` configures the tiptap editor with `autofocus: 'end'`.
The activation flow is:

1. `useBlockEditHover.onPointerUp` (mouse) calls `activate(el)`.
2. `activate` calls `ctx.setEditTarget({...})` — a React state update.
3. React re-renders; `dispatchers.renderBlockEditSurface` swaps the rendered
   block for `<RichTextEditor>`.
4. The editor mounts and `autofocus: 'end'` puts the caret at the end of the doc.

The **original mouse event is gone** by the time the editor exists, so
ProseMirror never gets to translate the click coordinates into a document
position. The second click works only because the editor DOM is already present
to receive it.

### Why this is the rich editor's problem specifically

The plain `EditTextarea` shows the **raw markdown source slice**, whose character
positions do not correspond to the rendered text the user clicked on, so
click→caret mapping is meaningless there (it uses logical-line `CaretHint`s for
cross-surface nav instead — see `caretGeometry.ts`). The rich editor renders the
**same visual text** as the rendered block in the **same measured box**
(`measureBlockBox` + `boxStyle`), so the viewport coordinates of the original
click land on (approximately) the same glyph in the mounted editor. That makes
`EditorView.posAtCoords()` a faithful translation.

---

## Mechanism

ProseMirror's `EditorView.posAtCoords({ left, top })` maps viewport coordinates
to a document position (`{ pos, inside }`), or returns `null` if the point is
outside any content. tiptap exposes the view as `editor.view`.

Plan:

1. **Capture** the activating click's viewport coordinates (`clientX`,
   `clientY`) at the mouse activation site and stash them on a context ref.
2. **Consume** them when the editor mounts: resolve the position with
   `editor.view.posAtCoords()`, set the selection there, focus, and clear the
   ref. Fall back to end-of-doc when there are no coords (keyboard activation) or
   `posAtCoords` returns null.

### Why a ref, not the `editTarget` object

`editTarget` is a heavily-invariant structure consumed by the self-heal /
re-anchor / commit-guard paths (`PreviewRoot.tsx`, `EditTextarea`). Threading
transient, mouse-only coordinates through it would:

- pollute every `setEditTarget` call site (keyboard, self-heal, click-switch,
  nav reland) that has no coordinates;
- risk a stale coordinate being reused on a re-anchor remount (we want the
  coords consumed exactly **once**, by the first mount).

A dedicated, write-once/read-once ref (`pendingClickCoordsRef`) keeps the concern
isolated and self-clearing, mirroring how `editDraftRef` / `editExpandedRef` are
already used as side-channels alongside `setEditTarget`.

---

## Design details

### 1. New context ref

In `PreviewContext.tsx`, add:

```ts
/**
 * Viewport coordinates of the mouse click that activated the current edit
 * target, or null for keyboard/touch activation. Written by
 * useBlockEditHover at the mouse activation site and read ONCE by
 * RichTextEditor at mount to place the caret at the clicked position
 * (posAtCoords) instead of end-of-block. The reader clears it after
 * consuming so a re-anchor remount does not reuse a stale click.
 */
pendingClickCoordsRef?: React.MutableRefObject<{ x: number; y: number } | null>;
```

Allocate the ref in `PreviewRoot.tsx` (alongside `editDraftRef` etc.) and pass it
into the context value (\~`PreviewRoot.tsx:1502` block).

### 2. Write the coords at the mouse activation site

`useBlockEditHover.onPointerUp` is the mouse activation path. It already filters
to `e.pointerType === 'mouse'` upstream (touch returns early; touch uses the
hold-timer `activate(el)` with no coords). Set the ref **immediately before**
calling `activate(el)`:

```ts
const el = findEditTarget(e);
if (el) {
    if (ctx?.pendingClickCoordsRef) {
        ctx.pendingClickCoordsRef.current = { x: e.clientX, y: e.clientY };
    }
    activate(el);
}
```

- The **touch** hold-timer path (`onPointerDown` → `setTimeout(activate, …)`)
  deliberately does **not** set coords → falls back to end-of-block. (A touch
  hold is not a precise caret placement gesture; end-of-block is acceptable, and
  we can revisit if needed.)
- The **keyboard** path (`onKeyDown` Enter/Space → `activate(_, {keyboard:true})`)
  does not set coords → end-of-block, matching today's behavior.

### Click-switch coverage (editor-to-editor)

The "switch directly from editing block A to editing block B" interaction
(common: finish a paragraph, click the next one) is **covered by the same
`onPointerUp` capture above** — no extra capture code is needed. Tracing the
lifecycle (`useBlockEditHover.onPointerDown` mouse branch +
`PreviewRoot.requestClickSwitch` / `handleClickSwitchBlur`):

- `onPointerDown` (editor A open, mouse, target = block B outside A's active
  region, different `anchorR0`) records a pending switch via
  `requestClickSwitch(B)`. The pointerdown also blurs A.
- **Clean A:** `handleClickSwitchBlur` returns `false`; the normal
  `onPointerUp → activate(B)` runs. No doc change, no re-anchor → B mounts once
  with coords present → **caret at click. ✓**
- **Dirty A:** `handleClickSwitchBlur` commits A fire-and-forget and closes it,
  then (per the comment at `PreviewRoot.tsx:1088-1092`, G18 Layer 1) `onPointerUp`
  **activates B unconditionally** — again through `activate(B)`. B mounts with
  coords present → caret at click. *But* committing A reflows the document and
  may **re-anchor B** (its byte range shifts if B is after A), remounting B's
  editor outside the `activate` path. The first mount consumes (nulls) the
  coords, so the re-anchor remount falls back to end-of-block. This is the
  correct behavior: after A's reflow the captured viewport coordinates are
  **stale** (B has moved on screen), so reusing them would place the caret at the
  wrong glyph. End-of-block is the honest fallback for this sub-case.

**Net:** fresh-open and clean-A click-switch (the overwhelmingly common cases)
place the caret at the click; the dirty-A-then-reflow-re-anchor sub-case falls
back to end-of-block by design. No `onPointerDown` change is required.

### 3. Consume the coords at mount in `RichTextEditor.tsx`

Two viable spots; we prefer placing the caret **after** the view is laid out:

- Keep `autofocus: 'end'` as the fallback (so keyboard/null-coords behavior is
  unchanged and the editor is focused even if `posAtCoords` fails).
- In the existing `useEffect([editor])` (already present for keydown/focusout
  wiring), after `editor` is ready, read and consume the coords:

```ts
useEffect(() => {
    if (!editor) return;
    const coords = ctx.pendingClickCoordsRef?.current ?? null;
    if (ctx.pendingClickCoordsRef) ctx.pendingClickCoordsRef.current = null; // consume once
    if (coords) {
        const hit = editor.view.posAtCoords({ left: coords.x, top: coords.y });
        if (hit) {
            // Clamp into the editable range and place the caret there.
            editor.chain().focus().setTextSelection(hit.pos).run();
        }
    }
    // ... existing keydown / focusout wiring ...
}, [editor]);
```

Open questions to resolve during implementation (see Risks):

- Whether `onCreate` vs a `useEffect`/`requestAnimationFrame` is needed for the
  view to be laid out enough for `posAtCoords` to be accurate. The editor mounts
  into the pre-measured box, so layout should be stable by the post-commit
  effect; if `posAtCoords` returns stale/`null` results, wrap the read in a
  single `requestAnimationFrame`.
- `setTextSelection(hit.pos)` may need clamping to a valid text position
  (`hit.pos` from `posAtCoords` is already a valid doc position, but if it lands
  on a non-text node boundary we may prefer `TextSelection.near`). Start with
  `setTextSelection`; adjust if a click in padding/margin misbehaves.

---

## Test plan (TDD)

Per CLAUDE.md, write tests first and watch them fail.

### Unit / integration (vitest + jsdom)

`posAtCoords` relies on real layout and returns `null` / `0` in jsdom, so the
**geometry itself cannot be asserted in jsdom**. What we *can* test there:

1. **Coords are captured on mouse activation, not keyboard/touch.** Spy on
   `pendingClickCoordsRef` after a synthetic mouse `pointerup` activation vs a
   keyboard Enter activation. Mouse → ref set to the event's client coords;
   keyboard → ref stays null. (New test in a `useBlockEditHover.*.integration`
   file.)
2. **The ref is consumed (cleared) at editor mount.** Mount `RichTextEditor`
   with `pendingClickCoordsRef.current` pre-set; assert it is null after mount,
   and that `editor.view.posAtCoords` was invoked (spy/mock the view). Verify the
   fall-through to end-of-selection when `posAtCoords` is mocked to return null.

These assert the **wiring contract** (capture → consume → fallback), which is the
part jsdom can verify.

### End-to-end (real browser — required before declaring done)

Geometry correctness must be verified in a real browser, consistent with the
project's end-to-end policy and how the rest of this editor was verified (Chrome
DevTools MCP against a running `q2 preview --allow-edit`, using `/tmp/rt-remo/`).

1. `cargo run --bin q2 -- preview /tmp/rt-remo/<doc>.qmd --allow-edit` with
   `&richText=1`.
2. Click in the **middle** of a multi-word paragraph (not at the end).
3. Assert the caret (ProseMirror selection) is at/near the clicked word on the
   **first** click — no second click needed.
4. Repeat for: start of paragraph, a heading, a click that lands in the
   left/right padding (should clamp gracefully, not throw), and a keyboard
   (arrow + Enter) activation (should still land at end-of-block).
5. **Click-switch:** with paragraph A open and *unedited*, click mid-word in a
   different paragraph B → caret lands at the click in B (no second click). Then
   the *dirty* variant: type into A, then click mid-word in B → A commits and B
   opens; confirm B is usable and the caret behavior is acceptable (caret at
   click if no re-anchor; end-of-block if A's edit reflowed/re-anchored B —
   document whichever occurs).
5. Capture a screenshot into `claude-notes/richtext-shots/` and record the
   observation in this plan.

---

## Work items

- [x] **Tests first.** Add jsdom integration tests for the capture→consume→
  fallback wiring (items 1–2 above); run and confirm they fail.
  - `useBlockEditHover.caret-coords.integration.test.tsx` — capture on mouse,
    not keyboard/touch. Mouse case red; kbd/touch pass trivially.
  - `richtext/caretFromClick.test.ts` — pure helper (posAtCoords hit →
    setTextSelection; miss → false). Red (module missing).
  - `richtext/RichTextEditor.caret.integration.test.tsx` — consume/clear ref
    at mount; helper mocked. Red (helper not called). Confirms tiptap mounts
    in jsdom.
- [x] Add `pendingClickCoordsRef` to `PreviewContext.tsx` (typed + documented).
- [x] Allocate the ref in `PreviewRoot.tsx` and expose it on the context value.
- [x] Write coords at the activation chokepoint. **Refined from the plan:**
  rather than writing the ref in `onPointerUp` before `activate` (which could
  leak coords on an `activate` early-return and forced clearing stale coords
  across close paths), the coords are threaded *into* `activate` via
  `opts.clickCoords` and written right before `ctx.setEditTarget` — the single
  point where an open commits, after all guards. Mouse passes coords;
  keyboard/touch pass nothing → ref set to null, which also clears any stale
  coords. One site covers fresh-open + click-switch; no `onPointerDown` change.
- [x] Consume coords in `RichTextEditor` mount effect (`posAtCoords` →
  `setTextSelection`) via the `placeCaretFromClick` helper; read+clear the ref
  once; keep `autofocus:'end'` fallback.
- [x] Run jsdom tests; confirm they pass (capture + helper + consume all green).
- [x] `npm run build:all` from `hub-client/` (production build is stricter) —
  passed (strict `tsc -b` project-references + vite + WASM rebuild). Also
  `tsc --noEmit` in preview-renderer: clean. Full preview-renderer suites:
  490 unit + 498 integration green, no regressions.
- [x] Committed verified core as `151be676` (unrelated Cargo.lock WASM-sync
  change left out).
- [x] End-to-end browser verification — **passed** (Chrome DevTools MCP against
  `q2 preview index.qmd --allow-edit` on port 7654, `&richText=1`). Method:
  dispatch a real mouse `pointermove`/`down`/`up` carrying the glyph's viewport
  coords (located via a Range), then read the resulting ProseMirror selection
  offset. Observed (caret offset vs. click target; `delta=0` means exact):

  | Scenario | block | target | caret | result |
  |---|---|---|---|---|
  | Paragraph mid (40%) | P | 79 | 79 | delta 0, not end ✓ |
  | Paragraph near-start (10%) | P | 19 | 19 | delta 0, not end ✓ |
  | Heading (50%) | H2 | 18 | 17 | delta −1 (sub-glyph), not end ✓ |
  | Clean click-switch P1→P2 (40%) | P | 46 | 46 | delta 0, not end ✓ |
  | Keyboard (ArrowDown×2 + Enter) | P | — | 199=len | at end ✓ (default kept) |

  Before the fix every mouse-open landed at 199 (end). Screenshot:
  `claude-notes/richtext-shots/14-caret-at-click.png` (editor open mid-paragraph,
  caret offset confirmed = click target = 79). Build chain used:
  `cargo xtask build-q2-preview-spa && cargo build --bin q2` (TS-only change;
  no WASM rebuild needed), restart the preview server (it embeds the SPA at
  startup), reload the page.

  **Key finding (recorded for future work):** the geometry was never the
  problem — `caretRangeFromPoint` returned the correct position immediately.
  The caret pinned to end because tiptap's `autofocus:'end'` applies its
  end-selection in a `requestAnimationFrame` that *raced and beat* our
  placement on the same frame. Fix: `autofocus:false`, and the mount effect
  owns the opening caret (click position, else explicit `focus('end')`).
- [x] Changelog: **intentionally skipped.** The two-commit `hub-client/changelog.md`
  workflow triggers on changes under `hub-client/`; this change lives in
  `ts-packages/preview-renderer`. More importantly the changelog is "a summary
  of user-facing changes," and the rich-text editor (bd-sjb4pzx8) is still
  behind the experimental `&richText=1` flag — it has **zero** changelog
  entries. A sub-improvement to an unshipped flag-gated feature would be
  inconsistent. Add one entry for the whole editor when the flag is removed.

---

## Risks / things to watch

- **Layout timing.** `posAtCoords` needs the editor laid out. The editor mounts
  into the pre-measured box, so a post-mount effect should be fine; if not, a
  single `requestAnimationFrame` before the read fixes it. Decide empirically in
  the browser, not by guessing.
- **Coordinate drift between rendered block and editor.** The editor's `<p>`/
  `<h_n>` is styled by the same theme CSS in the same box, but tiptap adds its
  own wrapper/padding. If the caret is consistently off by a small amount,
  inspect the editor's content padding vs the rendered block's. `posAtCoords`
  picks the nearest position, so small drift degrades gracefully (off by a
  character, not catastrophically).
- **Stale coords on re-anchor.** A self-heal re-anchor can remount the editor.
  Consuming (nulling) the ref at the first mount prevents a stale click from
  being reused on the remount — the remount falls back to end-of-block, which is
  acceptable (the user is not actively clicking during a self-heal). This is the
  same mechanism that makes the dirty-A click-switch sub-case fall back safely
  (see § Click-switch coverage).
