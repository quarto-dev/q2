# Flaky preview-renderer run: unhandled `elementFromPoint` error (bd-cpyq99ps)

**Date:** 2026-07-28
**Braid:** bd-cpyq99ps (bug, P2) — `discovered-from: bd-dofxhzaj`
**Branch:** `braid/bd-cpyq99ps-flaky-preview-renderer-integration` off `main` @ `7ff19ff8`

## Symptom

`cargo xtask verify` step 11 fails **with every test passing**:

```
 Test Files  48 passed (48)
      Tests  565 passed | 1 skipped (566)
     Errors  1 error
```

Vitest counts an *unhandled* error as a run failure, so a fully green suite
reddens the whole verify. Observed once during bd-dofxhzaj's pre-push verify on
a loaded machine; three immediate re-runs were clean.

```
TypeError: (intermediate value).elementFromPoint is not a function
 ❯ posAtCoords          node_modules/prosemirror-view/dist/index.js:465:10
 ❯ EditorView.posAtCoords  …:5724:16
 ❯ placeCaretFromClick  src/q2-preview/richtext/caretFromClick.ts:32:29
 ❯ src/q2-preview/richtext/RichTextEditor.tsx:235:22
 ❯ runAnimationFrameCallbacks  node_modules/jsdom/lib/jsdom/browser/Window.js:662
```

## Root cause

ProseMirror's `posAtCoords` does:

```js
let elt = (view.root.elementFromPoint ? view.root : doc)
            .elementFromPoint(coords.left, coords.top);
```

The ternary guards `view.root` but **not** the `doc` fallback. jsdom implements
`elementFromPoint` on neither, so the fallback branch throws.

Two things turned that into a *flake* rather than an honest failure:

1. **It runs in a `requestAnimationFrame`.** `RichTextEditor.tsx:227` schedules
   the opening-click replay a frame after mount (deliberately — the editor box
   must be laid out first). When the frame fires while its test is still
   running, the throw is attributed to that test; when it fires after the test
   finished, it escapes as an unhandled error belonging to no test. Which one
   happens depends on machine load — hence intermittent, and hence why it
   showed up during a verify competing with concurrent cargo builds.
2. **Nothing ever tested the real code path.** `caretFromClick.test.ts` drives a
   *fake* editor whose `posAtCoords` is a `vi.fn()`.
   `RichTextEditor.caret.integration.test.tsx` `vi.mock`s the whole
   `./caretFromClick` module. The file that actually tripped it,
   `p3-4-inline-breadcrumb.integration.test.tsx`, mocks nothing at all and
   mounts a real editor — it hit the real ProseMirror path by accident, not by
   design.

So `caretFromClick.ts`'s own comment — *"jsdom returns null from posAtCoords"* —
was **false**, and had never been checked.

## What was already fine (checked, not assumed)

The strand's original guesses about the production code were wrong, and are
recorded here so nobody re-litigates them:

- The mount effect **already** cancels its frame on cleanup
  (`return () => cancelAnimationFrame(raf)`) and **already** guards
  `if (editor.isDestroyed) return`. There is no leaked-callback bug to fix.
- `caretFromPoint` — the other coordinate helper on the same ProseMirror path —
  *does* guard both `doc.caretPositionFromPoint` and `doc.caretRangeFromPoint`
  with `if (doc.X)`. `elementFromPoint` is the single unguarded one.

## Fix

Stub `Document.prototype.elementFromPoint` to return `null` in
`src/test-utils/setup.ts`.

**Why the test environment and not the production code.** Every real browser
implements `elementFromPoint`; a `typeof … === 'function'` guard in
`caretFromClick.ts` would be dead code in production whose only purpose is to
paper over a test-harness gap. The honest jsdom answer is that no element is
under any point (there is no layout), and `null` says exactly that.
`posAtCoords` then takes its "outside the editor" branch and returns `null` —
which is the miss that `caretFromClick.ts` documents and its callers already
handle by falling back to `focus('end')`. The fix makes the documented contract
*true* rather than suppressing its violation.

**This is a precedent, not a new pattern.** `setup.ts` already carries a
structurally identical block: jsdom implements `getClientRects` on neither
`Text` nor `Range`, so ProseMirror's `coordsAtPos` threw from tiptap's autofocus
rAF, and it was fixed by stubbing zero-size rects. Same gap, same rAF, same
remedy — the new block sits directly beneath it and says so.

## Work items

- [x] Reproduce deterministically — new `caretFromClick.integration.test.ts`
      drives a **real** tiptap editor (real ProseMirror `EditorView`, nothing
      mocked). 5/5 tests fail before the fix with the exact
      `prosemirror-view:465` stack from the flake.
- [x] Fix: stub `Document.prototype.elementFromPoint` in `setup.ts`, beneath the
      existing `getClientRects` precedent, with a comment explaining the rAF
      escape mechanism.
- [x] 5/5 pass after.
- [x] Correct `caretFromClick.ts`'s comment: its jsdom claim is now true, and it
      points at *what makes it true* (the setup stub) and at the test that pins
      it.
- [x] Full preview-renderer integration suite: 4/4 clean runs, 49 files, 570
      tests, **0 errors**.
- [x] Unit suite unaffected: 40 passed / 2 skipped, 538 tests.
- [x] `cargo xtask verify --skip-hub-build` green (exit 0, all steps) — step 11
      is the one that flaked; it is the step this fixes
- [ ] PR

## Verification record

**Before the fix** (stub reverted via `git stash`), full integration suite ×6 —
the new test file fails every time, 5 `elementFromPoint` hits per run:

```
run 1: elementFromPoint-hits=5 | Test Files 1 failed | 48 passed (49)
… identical through run 6
```

**After the fix**, full integration suite ×4:

```
run 1: elementFromPoint-hits=0 | Test Files 49 passed (49) | Tests 570 passed | 1 skipped (571)
… identical through run 4
```

### Honest limit on what was reproduced

I reproduced and fixed the **root cause** deterministically. I did **not**
reproduce the original *intermittent manifestation* on demand: running
`p3-4-inline-breadcrumb.integration.test.tsx` alone ×5 and the full suite ×6
with the fix reverted never produced the unhandled-error form, because that
requires the rAF to lose its race against test teardown — which needs the
machine under load, as it was during the verify that first surfaced it.

That gap does not weaken the fix: the throw is now impossible regardless of
*when* the frame fires, since `posAtCoords` no longer has anything to throw
from. Timing only ever decided whether the error was attributed to a test or
escaped it.

## Follow-up worth considering (not done here)

`p3-4-inline-breadcrumb.integration.test.tsx` mounts a real `RichTextEditor`
and mocks nothing. That is how this was found, and it is a *good* property — but
it means any future unguarded browser API on the editor mount path will surface
the same way: as an unhandled error in an unrelated-looking test file. Worth
considering a vitest `onUnhandledError` that fails loudly with a pointer to
`setup.ts`, so the next one reads as "a DOM API is missing from the harness"
rather than "the breadcrumb test is flaky". Not filed as a strand — mentioning
it in case it earns its keep the next time this class of bug appears.
