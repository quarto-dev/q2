# Click-to-align: put the clicked block's source line at the same screen Y

## Overview

Clicking a block in the preview currently *reveals* its source line in Monaco —
centred in the q2-preview (`revealLineInCenterIfOutsideViewport`), centred in the
HTML preview (`revealRangeInCenter` inside `useSelectionSync`). Requested change:
**align**, don't reveal. Whatever height the clicked thing sits at on screen, its
first line of code should sit at that same height in the editor, so the two panes
line up across the split.

Validated interactively against a live local-prod hub on 2026-08-21/22 before
being written up; the working scratch is preserved at
`.superpowers/sdd/2026-08-21-preview-click-to-editor-scroll/validated-scratch-y-align.diff`
as the reference implementation for Phase 1.

Builds directly on `claude-notes/plans/2026-08-21-preview-click-to-editor-scroll.md`,
which established the click path, and inherits its decisions except where noted.

## The alignment computation

On screen a source line sits at `editorTop + (topForLine - scrollTop)`. To land it
at `hostY`, solve for the scroll position:

```
scrollTop = editor.getTopForLineNumber(line) - (hostY - editorTop)
            where editorTop = editor.getDomNode().getBoundingClientRect().top
```

Clamped to `[0, getScrollHeight() - getLayoutInfo().height]`. **Near the start or
end of the document the clamp wins and the panes will not line up exactly.** That
is geometry, not a defect — do not "fix" it by overscrolling.

**Two coordinate spaces.** The clicked element's `getBoundingClientRect()` is
relative to the *iframe's* viewport, so the iframe element's own offset in the
host page must be added. Getting this wrong misaligns by exactly the height of
whatever chrome sits above the preview — a constant offset is the symptom.

## Decisions

| # | Decision | Rationale |
|---|----------|-----------|
| A1 | **Align unconditionally**, even when the line is already visible. | `revealLineInCenterIfOutsideViewport` no-ops when the line is on screen. Alignment is a claim about *where* the line sits, so "visible somewhere" is not good enough. Cost: the editor moves on every block click. |
| A2 | **Measure the clicked block, not the edit region.** | At capture-phase `pointerup` the q2-preview edit region does not exist yet. It replaces the clicked block in place, so the block's top *is* the pane's top. If activation is ever changed to add chrome above the text, this becomes a small constant error and the fix is to measure post-activation and report the pane's real position back from the iframe. |
| A3 | **`hostY` is optional; absent ⇒ top-align.** | Keeps the signature additive and gives a defined behaviour for any caller that cannot measure. |
| A4 | **q2-preview keeps D1 (no cursor move, no focus steal); the HTML preview keeps its cursor move and focus steal.** | In q2-preview the same click opens an inline editor *in the preview*, so taking focus would break the gesture. The HTML preview has no inline editor, and its `setSelection` + `focus()` is the behaviour users have and like. Only the *reveal* changes there. |
| A5 | **Ratio scroll sync stays.** | It is governed by the existing scroll-sync UI toggle, so anyone who doesn't want the editor dragged along by preview scrolling turns it off. Removal was considered and rejected (braid **bd-mdcqnl84**, closed 2026-08-22, which also records that loc-based preview→editor sync was *specified* in the 2025-12-29 matched-scrolling plan and never implemented). `isSyncingRef`/U2e exist only because ratio sync can overwrite an alignment, so they stay too. |
| A6 | **Click-align is deliberately NOT gated by the scroll-sync toggle.** | `revealEditorLine` has no `enabledRef` check, while `syncPreviewToEditor`, `flushPendingScroll` and `scrollToLineDeferred` all do. This began as an accident but is the right behaviour and is now relied on: turning the toggle off gives you *only* click-align, with no ambient scroll coupling in either direction — which is how the feature is best evaluated and how at least one user works. It also matches D1's existing reasoning that an explicit click is unambiguous intent, unlike ambient sync. Pinned by row A1h so nobody "fixes" it by adding a gate. |

## Phase 1 — q2-preview (commit 1)

Reference implementation: the saved scratch diff. It is known-good interactively;
it is **not** test-bound yet, which is this phase's real work.

### Tests first (RED before implementation)

`makeEditor` in `hub-client/src/hooks/useScrollSync.test.ts` needs
`getTopForLineNumber` and `getDomNode` before any of these can be written.

- [x] **A1a** — `revealEditorLine(73, hostY)` calls `setScrollTop` with the exact
      computed value `(topForLine - (hostY - editorTop), 1)`. Exact arithmetic,
      both arguments — a bare one-arg assertion fails on arity.
      *Revert:* drop the `hostY` term → centred/top value → RED.
- [x] **A1b** — clamped at **0** when the computation goes negative (block near
      the top of the document, pane low on screen).
      *Revert:* remove the lower clamp → negative `setScrollTop` → RED.
- [x] **A1c** — clamped at `getScrollHeight() - getLayoutInfo().height`.
      *Revert:* remove the upper clamp → RED.
- [x] **A1d** — `hostY` omitted ⇒ top-align (`setScrollTop(topForLine, 1)`), per A3.
      *Revert:* make the fallback centre instead → RED.
- [x] **A1e** — **replaces U2a.** U2a asserted
      `revealLineInCenterIfOutsideViewport(73)`, which this phase deliberately
      stops calling. Assert that method is now **never** called, so a revert to
      centring reddens rather than silently passing.
- [x] **U2b unchanged; U2c/U2d/U2e rewritten** — U2b's assertions never touched
      `revealLineInCenterIfOutsideViewport`, so it needed no change. U2c/U2d/U2e
      originally asserted that method too (the same false premise A1e already
      names for U2a — caught during Phase 1, not before). Rewritten to assert
      `setScrollTop` by value and call count instead of by method name (U2e in
      particular discriminates the suppressed echo from the resumed ratio sync
      by call count + distinct values — alignment 50 vs. ratio 300 — since both
      now go through the same method).
- [x] **A1h** — with `enabled: false`, `revealEditorLine(73, hostY)` **still aligns**
      (per A6). *Revert:* add an `enabledRef.current` gate to `revealEditorLine`
      → RED. This row is the only thing standing between A6 and a plausible-looking
      "consistency" fix. Confirmed by an explicit revert probe (already passed
      before any implementation change, since the gate never existed; adding it
      temporarily reddened the row, then it was removed again).
- [x] **A1f** — extend **U3** so the registration tuple assertion is unchanged but
      `onClickAtLine` is asserted to receive **both** the line and a numeric
      `hostY`. *Revert:* stop passing the second argument → RED.
- [x] **A1g** — Playwright: click a block and assert the clicked element's
      viewport Y and the target line's rendered Y in Monaco agree within a stated
      tolerance. **This is the only row that binds the two coordinate spaces**;
      jsdom has no layout, so it cannot catch an iframe-offset error. State the
      tolerance and why.

### Implementation

- [x] Widen `onClickAtLine` to `(line: number, hostY?: number) => void`
      (`Q2PreviewIframe`), and `onPreviewClickAtLine` likewise
      (`ReactRenderer` → `ReactPreview`). Thread the prop **before** changing the
      handler, or `tsc -b` is red between steps.
- [x] `Q2PreviewIframe`'s capture-phase `pointerup` handler: resolve the nearest
      `[data-loc]` block, add its `getBoundingClientRect().top` to the iframe
      element's own top, pass as `hostY`.
- [x] `useScrollSync`: `revealEditorLine(line, hostY?)` performs the computation
      above, keeping the `isSyncingRef` bracketing (A5 — ratio sync stays, so the
      overwrite race stays reachable).
- [x] Rewrite the doc comments the scratch left stale: `useScrollSync`'s header
      still says `revealEditorLine` "calls `editor.revealLineInCenterIfOutsideViewport`".
- [x] Delete every `TEMPORARY LOCAL EXPERIMENT` marker (none carried into the
      real implementation — it was written fresh from the plan, not by applying
      the scratch diff verbatim).

### Phase 1 verification evidence (2026-08-22)

Recorded here because the working reports live under `.superpowers/`, which is
gitignored and dies with the worktree. These are the measurements worth keeping.

**Commits:** `ec05241e8` (implementation + rows), `0df2d4f1e` (changelog),
`c9e77e866` (A1h + checklist), `8f58fd128` (A1a collision fix), `0e9201b8c`
(collision note).
991 hub-client unit / 112 integration / 131 wasm; preview-renderer 549 unit /
587 integration; typecheck and `typecheck:tests` clean; `build:all` clean;
Playwright 6/6.

**A1g's tolerance is 6px, justified by measurement, not feel.** Reverting
`revealEditorLine` to the old centring call and running A1g alone failed it by
**~13px** — comfortably outside the tolerance, so the row discriminates rather
than merely passing. It targets `Paragraph 20.` (source line 45 of 80),
deliberately away from the document's start and end where the clamp would mask a
wrong computation.

**300 is a poisoned expected value in `useScrollSync.test.ts`.** The harness's
`getPreviewScrollRatio` is a fixed `0.5` over a 1000/400 editor, so
`syncPreviewToEditor` always produces exactly `setScrollTop(300, 1)`. A1a
originally expected that same value and therefore passed for an implementation
that wrongly routed `revealEditorLine` through the ratio path — the very mutant
its name claims to catch. (It died at U2c, whose focus gate suppresses the ratio
call entirely, so the seam held; the row alone did not.) A1a now uses
`topForLine=400, editorTop=80, hostY=200` → **280**, proven by mutating the
implementation to route through the ratio calc and observing
`expected [280,1], received [300,1]`. No other row collides: A1b=0, A1c=600,
A1d=222, U2c/U2d/A1h=50, U2e's first assertion=50. **U2e's second assertion is a
deliberate 300** — that step is specifically testing the *resumed* ratio path
after the suppression window elapses, so there 300 is the correct expected value,
not a collision. **If you change the editor fake's geometry (`getPreviewScrollRatio`
0.5, editor 1000/400), re-check every one of these.**

**A1h could not be validated by natural TDD sequencing**, because A6 was already
true of the implementation — there is no gate to remove. It was validated by
*adding* `if (!enabledRef.current) return;` to `revealEditorLine`, observing A1h
redden, then reverting. For a row that pins the *absence* of something, that is
the only meaningful proof.

**U2e's discrimination changed shape in this phase.** It previously separated "the
reveal" from "the ratio overwrite" by *method name* — the reveal called
`revealLineInCenterIfOutsideViewport`, the overwrite called `setScrollTop`. Both
now go through `setScrollTop`, so it discriminates by **value and call count**
instead: one call with the alignment value, still one after a scroll inside the
suppression window, two after the window elapses. That is strictly stronger —
value-discrimination survives an implementation swapping methods; method-
discrimination did not.

## Phase 2 — HTML preview (commit 2)

The HTML preview reaches the editor through a *different* path, and it is already
doing more than the q2 one: `useSelectionSync.handlePreviewSelection` runs on
`selectionchange` with **no collapsed-selection guard**, so a plain click already
does `setSelection` + `revealRangeInCenter` + `focus()`. (Measured in the previous
plan: clicking `Paragraph 35.` moved the editor to lines ~45–77 and Monaco took
focus.) So this phase **replaces the centring**, and per A4 leaves the cursor move
and focus alone.

- [ ] **Investigate first, and report before implementing:** `MorphIframe`'s
      `onSelectionChange` reports only `(startPos, endPos)` — no anchor rect. Decide
      where the anchor Y comes from. Candidates: widen `onSelectionChange` to carry
      it (symmetric with `onClickAtLine`), or compute it in `useSelectionSync` from
      the `previewRef` handle. Note the HTML writer stamps `data-loc` on **inlines**
      too, so the anchor may be a `<span>` rather than the block — decide whether to
      anchor on the innermost span or its containing block, and say why.
- [ ] **There are no `useSelectionSync` tests at all.** Create the file. Tier note:
      `*.test.ts` runs in the **node** environment, so a DOM fixture needs either
      `*.integration.test.ts` or a `@vitest-environment jsdom` docblock pragma.
- [ ] Rows: alignment arithmetic (as A1a), both clamps, and — importantly — that
      `setSelection` and `focus()` are **still called** (A4), so this phase cannot
      accidentally import q2's no-focus rule into the HTML preview.
- [ ] A Playwright row equivalent to A1g on the HTML preview path. T3 in the
      existing spec is the HTML-preview control and must stay green.
- [ ] Reuse `lineForClickTarget`/`parseDataLoc` rather than adding a parallel
      resolver, but note its `fileId !== 0` and `#q2-active-edit-region` guards were
      written for q2-preview — check each still makes sense on this path and say so.

## Verification (both phases)

- [x] `npm run test` + `npm run test:integration` in `hub-client` and
      `ts-packages/preview-renderer`; `npm run typecheck:tests`; `npm run build:all`.
      (Phase 1: hub-client 991 unit + 112 integration + 131 wasm passed, typecheck
      clean, `build:all` green; preview-renderer 549 unit/36 skipped + 587
      integration passed with the known baseline failure below, typecheck +
      typecheck:tests clean.)
- [x] Playwright spec green. (6/6, including the new A1g row, against a real
      `VITE_E2E=1` build.)
- [x] Executed fail-on-revert for every row above that names one. (A1a-A1e,
      U2c/U2d/U2e, A1f: observed RED against the pre-fix implementation before
      writing it, then GREEN after. A1g and A1h needed explicit revert probes
      since they postdate the implementation — see phase1-report.md.)
- [x] `hub-client/changelog.md` entry per commit that touches `hub-client/`
      (two-commit workflow). **Both** phases touch it. (Phase 1: `ec05241e8` /
      `0df2d4f1e`.)
- [x] TypeScript-only; `cargo xtask verify` not required. (No Rust changed.)

## Known baseline failure

`ts-packages/preview-renderer` `custom-components.integration.test.tsx >
Equation > appends \tag{N}` fails under the pinned katex 0.18.1 — braid
**bd-s36g9dav**, unrelated, fails identically before and after.
