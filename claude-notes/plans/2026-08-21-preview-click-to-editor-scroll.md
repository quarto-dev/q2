# q2-preview: clicking a block to edit does not scroll the source editor

## Overview

**Symptom (reported).** In the HTML preview, clicking in the preview scrolls the
Monaco source editor to the corresponding position. In **q2-preview**, clicking a
block to edit it does nothing to the editor. Editor→preview sync (cursor moves
scroll the preview) works in both.

**Root cause (confirmed).** Two independent facts compose into the bug:

1. `Q2PreviewIframe` listens for **`click`** on the iframe document to drive
   preview→editor sync (`ts-packages/preview-renderer/src/iframe/Q2PreviewIframe.tsx`,
   the `doc.addEventListener('click', handleClick)` in the `iframeReady` effect),
   the same event the HTML preview's `MorphIframe` uses.
2. q2-preview activates block editing on **`pointerup`** (`useBlockEditHover.tsx`,
   `onPointerUp` → `activate(el, {clickCoords})`), and activation **replaces the
   clicked element's subtree** with a synthetic edit region
   (`dispatchers.tsx::renderMeasuredEdit` → `<div id="q2-active-edit-region">`
   + textarea/tiptap). The clicked node is detached during `pointerup`.

Chromium does not dispatch a `click` event at all when the `pointerup` target has
been removed from the document during `pointerup`. Verified with a minimal
Chromium probe (host `<p>` replaced in a bubble-phase `pointerup` handler):

```
--- after clicking the REPLACED element:
    pointerup, target=target                      # no `document click` line
--- after clicking the INERT element:
    pointerup, target=inert
    document click, target=inert connected=true
```

So `onClick` → `handlePreviewClick` → `syncPreviewToEditor` never runs for the
one gesture that matters in q2-preview: clicking a block to edit it. The HTML
preview never mutates its DOM on click, so its `click` listener always fires.

**Reproduced in the real app** (2026-08-21) by
`hub-client/e2e/q2-preview-click-to-editor-scroll.spec.ts`, run against a live
hub + real Chromium:

```
T1 — clicking a block to edit delivers no click event to the q2-preview document   FAILED
     afterActivation === []   (in-region control click DID reach the document)
T2 — clicking a block does not scroll the editor to that block line                FAILED
     clicked `Paragraph 35.` (source line 73 of 78); editor still showed 2..19
T3 — control: the same click DOES reach the HTML preview document                  PASSED
  2 failed, 1 passed
```

### What the HTML preview actually does (corrects an earlier reading)

The HTML preview's *precision* does **not** come from the click→ratio path. It
comes from **`useSelectionSync`** (`hub-client/src/hooks/useSelectionSync.ts`:
`setSelection` + `revealRangeInCenter` + `focus()`), driven by a
**`selectionchange`** listener in `MorphIframe` that reads per-inline
`<span data-loc>` emitted by the native HTML writer. Measured in a real browser:

```
clicked `Paragraph 35.` (source line 73 of 78) in the HTML preview
  preview scroll ratio ...... 0.17          ← what ratio matching would use
  iframe events ............. ["click", "selectionchange"]
  editor moved to ........... lines ~45..77 (contains line 73)
  document.activeElement .... native-edit-context   ← Monaco took focus
  clicked DOM ............... <p data-loc="0:73:1-74:1">
                                <span data-loc="0:73:1-73:10">Paragraph</span> …
```

An editor at ratio 0.17 would be near line 13. It went to line 73. So the
mechanism that makes the HTML preview feel right is loc-based selection sync;
the click→ratio path contributes nothing precise. `Q2PreviewIframe` has **no**
`onSelectionChange` at all, and q2-preview stamps `data-loc` on **blocks only**
(bd-9kzfi scoped out inline granularity), so selection sync cannot be reused
as-is — see the decisions below.

**Why the test suite is green.** `useScrollSync.test.ts` covers only
editor→preview (7 tests, all `scrollPreviewToLine`); no test exercises
preview→editor click sync on either path. The jsdom tier *cannot* catch this
class of bug — a jsdom test dispatches `click` directly and so never reproduces
the browser's suppression of a click whose target was detached. Real-browser
tier is mandatory here. The originating plan
(`claude-notes/plans/2026-05-29-q2-preview-scroll-sync.md`, bd-9kzfi) explicitly
recorded "**NOT verified:** live browser scroll interaction".

## Decisions (settled 2026-08-21 — these bind the frozen rows below)

| # | Decision | Rationale |
|---|----------|-----------|
| D1 | **Scroll only.** `revealLineInCenterIfOutsideViewport(line)`. No `setPosition`, no `setSelection`, no `focus()`. | In q2-preview the click *opens an inline editor in the preview*. Pulling focus to Monaco (what `useSelectionSync` does for the HTML preview) would break the gesture the same click just started. Deliberate divergence from the HTML preview. |
| D2 | **New capture-phase `pointerup` path**, not an extension of `useSelectionSync`. | Works with the block-level `data-loc` q2-preview already stamps. Extending selection sync would require per-inline `<span data-loc>` in q2-preview, reopening the wrapper/theme-CSS parity decision bd-9kzfi deliberately closed. |
| D3 | **HTML preview untouched.** Its click→ratio path stays, redundant but harmless. | It is the behaviour reported as working; changing it risks the one preview that currently syncs. Recorded as a finding, not a work item. |
| D4 | **A click that does not resolve an editable block does nothing.** No reveal when the target is inside `#q2-active-edit-region`, when the nearest `[data-loc]` ancestor is a `<section>`, or when nothing resolves. | Without this, every caret-move click inside an open editor yanks Monaco to the enclosing section's heading — see the hazard below. Mirrors the existing active-region guard in `useBlockEditHover`'s `onPointerUp`. |
| D5 | **Clicking included content scrolls to the `{{< include … >}}` shortcode's line in the current file.** Not a bogus current-file line; not a file switch. | Keeps the editor showing the file the user is editing, and points at the thing that *is* editable there. Feasibility + sequencing: see Phase 4. |

### The section hazard behind D4

`closest('[data-loc]')` does **not** return "nothing" inside an open editor.
`renderMeasuredEdit` nests the edit region inside `AttributionWrap` (which emits
no `data-loc`), but the enclosing **`<section>` does** carry one:
`q2-preview/blocks/Div.tsx` spreads `dataLocProps` onto section Divs, and
`SectionizeTransform` is unconditional for non-reveal formats
(`crates/quarto-core/src/pipeline.rs`, the `else` branch that pushes
`TitleBlockTransform` + `SectionizeTransform`). So in any document **with a
heading**, a click inside the open textarea — or on inter-block whitespace —
resolves the section's `startLine` and would scroll Monaco to the heading.

The current E2 fixture has **no headings**, so it cannot see this. Fixing that
is a work item (Phase 1: add a heading to the fixture, add E4).

## Design

Switch preview→editor **click** sync from "`click` + ratio" to
"**capture-phase `pointerup`** + `data-loc`".

**Why capture phase:** it runs before the app's bubble-phase activation handler,
so the clicked element is still attached and its nearest `[data-loc]` ancestor is
resolvable. Verified in the same Chromium probe:
`CAPTURE pointerup: target=target connected=true data-loc=0:12:1-14:20`
followed by `BUBBLE pointerup (app activate): replacing node`.

**Why `pointerup` and not `pointerdown`:** `pointerup` is where a drag-select
*ends*, so the reveal follows the user's final position rather than firing at the
start of a drag; and it is the same event the app activates on, so the reveal
coincides exactly with the editor opening. (An earlier draft justified this by
`syncPreviewToEditor`'s focus gate — that reason does not survive D1's
"not focus-gated", and a capture-phase `pointerdown` is equally before-detach.)

**Why not focus-gated:** an explicit click in the preview is unambiguous user
intent. The focus gate exists to break *scroll* feedback loops; this path is not
one.

**Why no cursor move:** `setPosition` fires `onDidChangeCursorPosition`, which
feeds editor→preview sync and would bounce.

### Pinned API (frozen — U1/U2/U3 assert against these exact names)

```ts
// ts-packages/preview-renderer/src/iframe/scrollSyncDom.ts   (new export)
/**
 * The editor line a preview click should reveal, or null when the click should
 * be ignored. Named for the click path, not as a generic DOM helper, because
 * the ignore rules (D4) are part of its contract.
 */
export function lineForClickTarget(target: EventTarget | null): number | null;
//   null  — target is not an Element
//   null  — target is inside #q2-active-edit-region
//   null  — nearest [data-loc] ancestor is a <section>
//   null  — no [data-loc] ancestor
//   else  — startLine of the nearest [data-loc] ancestor (innermost wins)

// ts-packages/preview-renderer/src/iframe/Q2PreviewIframe.tsx   (prop change)
/** Called on a preview click that resolves to a block, with its start line. */
onClickAtLine?: (line: number) => void;   // replaces `onClick`; only fires with a number
// listener: doc.addEventListener('pointerup', handler, /* capture */ true)

// hub-client/src/hooks/useScrollSync.ts   (new returned callback)
/** Reveal `line` in the editor. No debounce, not focus-gated, no cursor move. */
revealEditorLine: (line: number) => void;

// ts-packages/preview-renderer/.../ReactRenderer.tsx  +  hub-client/.../ReactPreview.tsx
onPreviewClickAtLine?: (line: number) => void;   // replaces `onPreviewClick` on the q2 branch
```

`handlePreviewClick` (ratio) stays in `useScrollSync` — the HTML preview still
uses it (D3). q2-preview stops wiring it.

### Deferred refinement — line *within* the block

When the active surface is the **plain textarea** (not tiptap), the caret offset
within the draft gives a line offset inside the block: editor line ≈
`blockStartLine + linesBefore(caretOffset)`. This needs data only the iframe app
has, so it rides a new `EDIT_CARET`-style postMessage rather than the
parent-side DOM listener. Deliberately deferred — the primary ask is "scroll to
a line that's in the element being edited".

## Test Seam Spec (frozen — prevalidating-test-seams)

One row per test: tier · real unit mounted · seam (mount + events + assertion
surface) · mock boundary · the named production hunk whose revert reddens it.
Once a row goes green its assertions and harness are **frozen** — never edited
to go green. (Sole exception: E1's polarity flip, below.)

### Tier note — which vitest project a row lands in

Both packages run `*.test.ts` in the **node** environment
(`ts-packages/preview-renderer/vitest.config.ts`,
`hub-client/vitest.config.ts`) and `*.integration.test.ts` in **jsdom**
(the `vitest.integration.config.ts` in each). A DOM-fixture test in a
`*.test.ts` has no `document`. Two escapes: put it in
`*.integration.test.ts`, or add a `@vitest-environment jsdom` docblock pragma
(which is what `hub-client/src/hooks/useScrollSync.test.ts` already does).

| # | Tier | Real unit mounted | Seam: mount + events + assertion surface | Mock boundary | Revert → RED |
|---|------|-------------------|------------------------------------------|---------------|--------------|
| **U1a** | jsdom (`scrollSyncDom.integration.test.ts` — existing file, jsdom config) | real `lineForClickTarget` | `<section data-loc="0:5:1-30:1"><p data-loc="0:12:1-14:20"><em id="t">x</em></p></section>`; call with `#t` | none (pure DOM) | Swap `el.closest('[data-loc]')` for a document-wide `querySelector('[data-loc]')` → returns 5 instead of **12** → RED |
| **U1b** | ↑ | ↑ | Element with no `[data-loc]` ancestor → `null` | ↑ | Add any "nearest located block in the document" fallback → RED |
| **U1c** | ↑ | ↑ | Target inside `<div id="q2-active-edit-region">` **nested in a located `<section>`** → `null` | ↑ | Delete the active-region guard → resolves the section's 5 → RED. **This is the D4 hazard row** |
| **U1d** | ↑ | ↑ | Target whose nearest `[data-loc]` is the `<section>` itself (inter-block whitespace) → `null` | ↑ | Delete the `<section>` check → RED |
| **U1e** | ↑ | ↑ | `lineForClickTarget(document)` and `(null)` → `null`, no throw | ↑ | Drop the `instanceof Element` narrowing → throws → RED |
| **U2a** | jsdom (`useScrollSync.test.ts`, `@vitest-environment jsdom` already present) | real `useScrollSync` | `renderHook`; `result.current.revealEditorLine(73)`; assert `revealLineInCenterIfOutsideViewport` called with **`73`** | Monaco fake (`makeEditor`) — **must be extended**, see below | Remove the reveal call → RED |
| **U2b** | ↑ | ↑ | `setPosition`, `setSelection`, `focus` **never** called | ↑ | Add `setPosition`/`focus` to the reveal path (i.e. copy `useSelectionSync`'s semantics) → RED. Guards D1 |
| **U2c** | ↑ | ↑ | **Harness must be `setup({focus: true})`**; reveal still happens | ↑ | Route `revealEditorLine` through `syncPreviewToEditor` (whose first statement is the focus gate) → RED |
| **U2d** | ↑ | ↑ | No debounce: reveal happens **without** advancing timers | ↑ | Wrap the reveal in the 50 ms `editorDebounceRef` timer → RED |
| **U3** | jsdom (`Q2PreviewIframe.integration.test.tsx`) | real `Q2PreviewIframe` | **New harness variant needed** — the existing `renderWithFingerprint` dispatches `IFRAME_READY` internally and exposes no handle on the iframe element. Need: render → wrap `iframe.contentDocument.addEventListener` with a spy → dispatch `IFRAME_READY`. Assert the registration tuple is `('pointerup', fn, true)`, then dispatch a `pointerup` on an injected `[data-loc]` node and assert `onClickAtLine` got `12` | iframe `contentWindow.postMessage` (already faked here) | Restore `doc.addEventListener('click', handleClick)`, or drop the `true` capture arg → RED |
| **U4** | jsdom (`useScrollSync.test.ts`) | real `useScrollSync` | `handlePreviewScroll()`; advance 50 ms; assert `setScrollTop` called with **`(300, 1)`** — ratio `0.5 × (1000 − 400)`; and with `setup({focus: true})` it is **not** called | Monaco fake | Delete the ratio body of `syncPreviewToEditor` → RED. **Missing-test-pass addition** — nothing covers preview-scroll→editor today, so this refactor could kill it silently |
| **E1** | Playwright (`q2-preview-click-to-editor-scroll.spec.ts` T1) | whole app | **Environment characterization, no production hunk** — see the exception note | none (real browser) | *(declared exception)* |
| **E2** | Playwright (same spec, T2) | whole app | Live hub + real Chromium; click `Paragraph 35.`; assert Monaco's rendered lines contain it. **Plus** (missing-test pass): the preview's `scrollY` is unchanged ±4px afterwards, so a reveal→cursor→scroll feedback loop reddens it | none | Any of: back to a `click` listener; drop the capture arg; route `onClickAtLine` into `syncPreviewToEditor` (ratio) instead of `revealEditorLine`; drop the `onPreviewClickAtLine` threading in `ReactRenderer`/`ReactPreview` → RED |
| **E3** | Playwright (same spec, T3) | whole app | HTML-preview control: the same click **does** reach the iframe document | none | *(control — no hunk; fails only if the harness itself breaks)* |
| **E4** | Playwright (same spec, **new**) | whole app | **Requires a heading in the fixture** (see D4 hazard). Click a paragraph to open its editor, then click *inside the textarea*; assert the editor's visible lines did not move | none | Delete the active-region guard from `lineForClickTarget` → the section's heading line is revealed → RED. Browser-tier because only a real activation detaches the block |

### Harness extensions required before these rows can be written

- `makeEditor` (`useScrollSync.test.ts`) has no `revealLineInCenterIfOutsideViewport`,
  `setPosition`, `setSelection` or `focus`, and `setup()` returns neither the
  editor nor its `setScrollTop` spy. U2a/U2b/U4 need all of that exposed. Extend
  the shared helper; do not fork it.
- Production calls `editor.setScrollTop(editorScrollTop, 1)`, so U4 must assert
  `(300, 1)` — a bare `toHaveBeenCalledWith(300)` fails on arity.
- U3 needs the new `Q2PreviewIframe` harness variant described in its row.

### Vacuity checks performed

- **E2 cannot be satisfied by ratio matching.** The fixture's trailing 6000px
  spacer puts the click target at preview scroll ratio < 0.25 while its source
  line is at > 0.9 of the document, and *both* are asserted as preconditions
  before the click. So E2 discriminates "loc-based reveal" from "the only
  mechanism that exists today", not merely "the editor moved". Verified RED with
  both preconditions passing.
- **Monaco renders spaces as ` `** (`hasNbsp: true`, measured). Before the
  fix, `innerText.includes('Paragraph 35.')` was `false` even with that line
  visibly on screen — which made E2's assertion unsatisfiable *and* its
  `not.toContain(...)` precondition vacuously true. `editorVisibleText` now
  normalises ` ` → space. Any new assertion on Monaco's rendered text must
  go through that helper.
- **U2c would be vacuous with the harness default.** `setup()` takes `focus` as
  a parameter; U2c is only meaningful with `focus: true`. Written into the row so
  the executor cannot pick the convenient default.
- **U3 cannot discriminate capture from bubble on its own.** jsdom computes the
  event path at dispatch and keeps propagating even if a listener detaches the
  target, so a *bubble*-phase `pointerup` listener passes U3's behavioural half
  just as well — i.e. the broken variant survives. That is why U3's binding
  assertion is the **registration tuple**, and why E1/E2/E4 are not optional.
  Recorded so nobody later "simplifies" U3 by deleting the tuple assertion.
- **U1's discriminators are exact line numbers**, not `!= null`. An
  implementation returning the outermost `[data-loc]` (5) fails U1a; one without
  the guards fails U1c/U1d with the section's 5.

### Declared exception (check #1)

**E1 has no production revert hunk.** It asserts a *browser* fact — Chromium
delivers no `click` when the `pointerup` target was detached during `pointerup`,
while a capture-phase `pointerup` listener on the same document still sees the
node attached with a parseable `data-loc`. Its job is to keep the design
rationale executable: if a future Chromium changes this, E1 goes RED and tells
us the `click` listener would have worked after all. Production binding for the
same behaviour is E2's job. Logged rather than faked.

**E1's polarity flips when the fix lands.** As shipped today it is written as the
*repro* (`expect(afterActivation).not.toEqual([])` — RED, proving the click never
arrives). At implementation time it must be rewritten to the characterization
form (`expect(afterActivation).toEqual([])` plus "a capture-phase `pointerup`
listener on the same document sees the target attached with a parseable
`data-loc`"). This is the one row exempt from "frozen once green", and the flip
is a Phase 2 checklist item — not a later cleanup, and not a deletion of E1.

### Missing-test pass (check #3)

| Gap | Disposition |
|-----|-------------|
| preview **scroll** → editor (ratio) has no test at all today; this refactor touches `syncPreviewToEditor`'s neighbourhood | **Specced as U4** (exact `(300, 1)`, plus the focus gate) |
| Caret-move click inside the open editor must not move the editor (the D4 section hazard) | **Specced as U1c (unit) + E4 (browser)**; E4 needs a heading added to the fixture |
| Inter-block whitespace / section-background clicks | **Specced as U1d** |
| Reveal must not start a feedback loop (reveal → cursor event → editor→preview scroll) | **Specced as the E2 addition** (preview `scrollY` unchanged ±4px) |
| Non-Element event targets (`document`, `null`) | **Specced as U1e** |
| Every other pointerup in the iframe also reaches the capture listener — `EditToolbar` buttons, links, comment UI, breadcrumb crumbs | **Covered by construction, not by a row:** those all sit inside either the active edit region (U1c) or a located block whose line is the right answer anyway. **Accepted-untested** for the toolbar/comment chrome specifically; revisit if a stray reveal shows up in the live pass |
| Keyboard activation (roving Enter/Space) and touch hold-to-edit produce no `pointerup`, so they get no reveal | **Accepted-untested, and a known behaviour gap of this design.** Covering it needs the deferred `EDIT_CARET`-style postMessage (which the iframe app must originate). Not silently omitted — if we want it now, the design changes from a parent-side DOM listener to a message |
| `format: revealjs` also routes through `Q2PreviewIframe` (`ReactRenderer`'s `format === 'q2-preview' \|\| 'revealjs'` branch), so clicking a slide will now reveal lines | **Accepted-untested.** Decks have their own cursor↔slide sync (`SET_SLIDE`/`SLIDE_CHANGED`); a line reveal is additive. Flagged for follow-up if it fights slide navigation |
| Included-file content (foreign `fileId`) | **Phase 4 investigation (2026-08-21) found this is NOT inert in Phase 2 as shipped — see the task-7 report** (`.superpowers/sdd/2026-08-21-preview-click-to-editor-scroll/task-7-report.md` §0): it reveals a wrong line. Bugfix (`U1f`/`U1g`) and the D5 enhancement (`U5a`–`c`/`E5`) are specced in Phase 4, follow-up implementation |

## Running these tests (fresh worktree)

Not obvious, and all of it is required before the Playwright rows can run:

```bash
npm install                                   # from the WORKTREE root, never hub-client/
for p in ts-packages/*/; do npm run build --if-present -w "$p"; done
                                              # e2e helpers import @quarto/quarto-sync-client/dist
cd hub-client && npm run build:wasm           # or copy crates/wasm-quarto-hub-client/pkg/* from
                                              # a warm checkout when no Rust changed
VITE_E2E=1 npm run build                      # without this, window.__quartoTest is tree-shaken
cd .. && cargo build --bin hub                # globalSetup's `cargo run --bin hub` has a 120s
                                              # deadline; a cold compile blows it
```

Ports 3031 (test hub) and 5174 (`vite preview`) must be free. Then:

```bash
cd hub-client
npx playwright test e2e/q2-preview-click-to-editor-scroll.spec.ts \
  --project=chromium --workers=1 --retries=0 --reporter=line
```

`npm install` in a worktree prunes other-platform optional deps from
`package-lock.json` — **revert that file**, never commit it.

## Work Items

### Phase 1 — tests first (RED). Rows refer to the Test Seam Spec above.
- [x] **E1/E2/E3** — spec written and run: E1 RED, E2 RED (preconditions green), E3 GREEN.
- [x] **E2 assertion surface** — normalise Monaco's ` `; re-run confirms E2 still RED with both preconditions meaningful.
- [x] **Fixture** — heading `# Section one` added, so `SectionizeTransform` produces a `<section>` and the D4 hazard is reachable. `paraLine(n)` is now `5 + 2n`, `totalLines` 80, and E2's `> 0.9` precondition re-verified numerically as 75/80 = 0.9375 (threshold unchanged). The stale "2500px / ratio 0.9" header comment turned out to have been fixed already in 650f6569a — nothing to do there.
- [x] **E2 addition** — preview `scrollY` unchanged ±4px after the click (`previewScrollY` helper, sibling to `previewScrollRatio`).
- [x] **E4** — caret-move click inside the open editor does not move the editor (spec T4). Green today by construction (nothing reveals yet); binding is post-fix via revert (c). Review confirmed the textarea is a DOM child of `#q2-active-edit-region` (`dispatchers.tsx:88`), so the row is not decorative.
- [x] **U1a–U1e** — `lineForClickTarget`, in the **integration** (jsdom) file. U1b initially had *no* `[data-loc]` anywhere in its fixture, so a document-wide-fallback implementation would also have returned `null` — caught in review and fixed with a non-ancestor decoy.
- [x] **U2a–U2d** — `revealEditorLine` semantics; `makeEditor`/`setup` extended in place with `revealLineInCenterIfOutsideViewport`/`setPosition`/`setSelection`/`focus` spies and an `editor` handle.
- [x] **U3** — registration tuple `('pointerup', fn, true)` + forwarded line. Harness refactored into shared `mountIframe()` + `dispatchIframeReady()` so the new spy variant is not a fork of `renderWithFingerprint`.
- [x] **U4** — preview-scroll→editor ratio regression cover, `setScrollTop(300, 1)` and the focus-gated negative. Green from the start (covers existing behaviour).

### Phase 2 — implement (items 1–3 are one compile unit; land them together)
- [x] `scrollSyncDom.ts`: add `lineForClickTarget` (with the D4 guards). **The Pinned API's `instanceof Element` narrowing was wrong** and would have shipped the feature dead — see the cross-realm note below. Ships duck-typed instead.
- [x] Thread `onPreviewClickAtLine` through `ReactRenderer` → `ReactPreview`
      **first** — swapping the listener before the prop exists leaves `tsc -b` red.
- [x] `Q2PreviewIframe.tsx`: replace `doc.addEventListener('click', …)` with the
      capture-phase `pointerup` listener; keep `onScroll` (window scroll → ratio) unchanged.
- [x] `useScrollSync.ts`: add `revealEditorLine`; keep `handlePreviewClick` for the HTML preview.
- [x] **Flip E1's polarity** in the same commit (see the declared exception).

### The cross-realm defect in this plan's own Pinned API (found 2026-08-21)

The Pinned API above specified `lineForClickTarget` narrowing its argument with
`target instanceof Element`. **That is wrong in production and would have shipped
this feature completely dead**, with every jsdom row green.

`lineForClickTarget` is called from the listener registered by `Q2PreviewIframe`,
which lives in the **parent frame's** JS realm, on a `pointerup` whose target is a
node from the **sandboxed iframe's** realm. Each realm has its own `Element`
constructor, so the parent's `instanceof Element` is `false` for every real
click — including a perfectly ordinary `<p>`. Measured in jsdom:

```
target is a real element  : EM
iframe-realm instanceof   : true
PARENT-realm instanceof   : false      <- what production evaluates
duck-typed closest        : true
closest([data-loc]) start : 0:12:1-14:20
```

Shipped narrowing is therefore duck-typed — `typeof target.closest === 'function'`
— which is realm-agnostic and still excludes `Document` and `Window` (neither has
`closest`). **Do not "tidy" it back to `instanceof Element`.**

Only the real-browser row (E2) caught this; all thirteen jsdom rows passed against
the broken guard. That is the plan's own "real-browser tier is mandatory here"
argument arriving in a form nobody predicted — and it is why **U1f** was added:
it reproduces the cross-realm case at the jsdom tier (a genuine iframe
`contentDocument`, not the main test document), so a revert to `instanceof
Element` now reddens a fast test instead of only Playwright. Verified both
directions: reverting the guard reddens *only* U1f; deleting the guard outright
still reddens U1e.

Related: **U3's assertion 2 could never have run as originally written.** It did
`doc.body.innerHTML = …` on a src-ful iframe, whose `contentDocument.body` is
`null` in jsdom (`readyState` stays `'loading'`). The row previously failed at
assertion 1 first, so the setup bug was latent. Fixed by injecting the fixture via
`open()/write()/close()` — the pattern `iframePostProcessor.integration.test.ts`
already used in that package — **before** the listener spy is installed, since
per spec `document.open()` wipes document listeners (jsdom merely happens not to
implement that). Assertions unchanged.

### Phase 3 — verify
- [x] `npm run test` + `npm run test:integration` in **both** `ts-packages/preview-renderer` and `hub-client`. (preview-renderer 549 unit / 587 integration; hub-client 986 / 112. One **pre-existing, unrelated** integration failure: `custom-components` `Equation > appends \tag{N}` under the pinned katex 0.18.1 — braid **bd-s36g9dav**, fails identically before and after every change here.)
- [x] `npm run typecheck:tests` (preview-renderer) — clean.
- [x] `cd hub-client && npm run build:all` — succeeded (chunk-size warnings only). Full `cargo xtask verify` was **not** required; the change stayed TypeScript-only as planned.
- [x] Playwright spec green — **5/5** (T1 characterization, T2 reveal, T3 HTML control, T4 D4 guard, T5 no-overwrite).
- [x] Fail-on-revert, **executed** not just documented — and it found two rows that were not bound. Six hunks (the plan anticipated three; (d)-(f) came from defects found during execution), each reverted individually and restored with `git checkout --`:
  - (a) capture arg `true` → `false` — U3 reddened. **Playwright T2 did NOT redden** (reproduced twice). See correction 1 below: the plan's claim that dropping the capture argument reddens E2 is false.
  - (b) `revealEditorLine` → `syncPreviewToEditor` — U2c + T2 reddened. Bound.
  - (c) delete the active-region guard — **neither U1c nor T4 reddened** (reproduced twice). Both rows were vacuous; both fixed, and each is now proven to redden in both directions.
  - (d) duck-typed guard → `instanceof Element` — U1f reddened, and *only* U1f among the 17 jsdom rows, which is exactly why U1f exists.
  - (e) remove the `isSyncingRef` bracketing — U2e + T5 reddened. Bound.
  - (f) remove the `fileId !== 0` guard — U1g reddened, and only U1g (U1h, the same-file control, stayed green, proving the guard is not over-broad).
- [x] Live browser session against a real local-prod hub, on a 292-line multi-element document: top/middle/bottom clicks all revealed the clicked block's line; a caret move inside an open editor left the editor unmoved; preview `scrollY` held constant across click→cursor-move→click (no oscillation). An element-kind sweep of nine block kinds passed 8/9 — **the one failure was a real bug and is fixed below**.
- [x] `hub-client/changelog.md` entry (two-commit workflow) — `ed6462682` for `6187ea9f5`.

### Phase 1 execution notes (2026-08-21)

Two plan corrections found while executing, recorded here rather than left to
bite a later reader:

- The Pinned API block above places `ReactRenderer.tsx` under
  `ts-packages/preview-renderer/`. It is actually
  `hub-client/src/components/render/ReactRenderer.tsx` (its `onPreviewClick`
  prop is declared ~line 138 and forwarded as `onClick={onPreviewClick}`
  ~line 337). Prop *names* in that block are correct and frozen; only the
  path was wrong.
- **Phase 4 is investigation-only in this pass.** Its producer-side option is a
  Rust + wire change, which would invalidate Phase 3's explicit "TypeScript-only,
  `cargo xtask verify` not required" contract and turn a bugfix branch into a
  wire change. Phase 2 already leaves foreign-`fileId` clicks inert, so nothing
  is blocked; D5's implementation becomes a follow-up.

Out-of-plan defect found and filed separately (**braid bd-s36g9dav**): in this
worktree `ts-packages/preview-renderer`'s
`custom-components.integration.test.tsx > Equation > appends \tag{N}` fails.
It is not a regression from this branch — root + sandboxed-preview `package.json`
and the lockfile all pin katex exactly **0.18.1**, under which KaTeX no longer
emits the `.tag` element the test queries. The main checkout only appears green
because its `node_modules` is stale at 0.17.0. The pin arrived via Snyk PR #571
(`669ad7534`), which did not update the assertion. Treat that row as an expected
baseline failure while working in a correctly-installed tree.

### Corrections to this plan, found by executing it (2026-08-21)

Three of this plan's own stated premises turned out to be false. Each had already
caused a real defect or a vacuous test, so they are recorded here: a wrong premise
left in place causes the next one.

**1. "Dropping the capture argument reddens E2." False.** Reverting the capture flag
reddens U3 but leaves Playwright T2 green (reproduced twice). The app's own
bubble-phase handler runs first and detaches the clicked node — but a *detached*
`<p>` still carries its own `data-loc`, and `closest('[data-loc]')` called on a
detached node finds itself, so the line still resolves. Capture phase remains the
right choice (it guarantees the target is attached, which matters when the resolved
*ancestor* rather than the target is what gets replaced), and **U3's
registration-tuple assertion is the only thing binding it** — do not delete that
assertion on the theory that the browser tier covers it.

**2. "The enclosing `<section>` carries a `data-loc`." False — and this was the whole
stated basis for D4.** The spread *is* unconditional, but its input never exists:

- `dataLocProps` (`framework/sourceLoc.ts`) returns `{}` for any node with no `l`.
- `crates/pampa/src/transforms/sectionize.rs` builds section Divs with
  `SourceInfo::Generated { by: By::sectionize(), from: smallvec![] }`.
- `quarto-source-map`'s own docs name **"sectionize wrappers"** as the canonical
  example of pure synthesis with no source-side preimage, and its `map_offset`
  returns `None` for `Generated` unconditionally.
- so `resolve_location` never emits an `l` for a section, and **no `<section>` in a
  rendered q2-preview document ever carries `data-loc`.** Confirmed both from that
  chain and by dumping a real ancestor chain in the live app
  (`section#callouts[data-loc=null]`).

Consequence: `lineForClickTarget`'s `<section>` null case is **dead code under
q2-preview today**, and U1d exercises a situation that cannot currently occur. The
guard is kept as defence-in-depth — one comparison, and correct if a future change
ever gives sections a resolvable location — but read the D4 rationale as "the
enclosing *located wrapper*", not "the section".

The hazard D4 really guards is a block inside a located **non-section** wrapper: a
fenced div (`Div.tsx` spreads `dataLocProps` on its plain-`<div>` branch too), a
column, and so on. **Callouts are not such a wrapper** — `Callout.tsx` never spreads
`dataLocProps` at all. This is why U1c's original fixture was vacuous: it nested the
edit region directly inside a located `<section>`, so with the active-region guard
deleted the *separate* section guard returned `null` anyway and the row could not
tell the two implementations apart. Measured:

```
edit region inside a located <section>  ->  null   (old fixture: passes with the guard DELETED)
edit region inside a located <div>      ->  20     (wrong line: discriminates)
```

A second, independent vacuity hazard surfaced while binding T4:
`revealLineInCenterIfOutsideViewport` is a **no-op when the target line is already
visible**, and a wrapper's start line sits only 1-3 lines from its child's — so
without first scrolling Monaco away, a missing guard's reveal would also no-op and
T4 would pass either way. T4 therefore scrolls the editor pane (a host-frame action;
the edit region lives in the sandboxed iframe, so it cannot close it) before the
final caret click.

**3. "Phase 2 leaves foreign-`fileId` clicks inert." False — they were wrong, not
inert.** `lineForClickTarget` parsed the `fileId` and never checked it, so clicking
included content revealed the *included* file's line number as a line in the
currently-open file: a real, editable, wrong line. Fixed on this branch with a fourth
null case (`fileId !== 0`), rows U1g/U1h. D5 proper — revealing the
`{{< include … >}}` shortcode's own line — remains deferred follow-up.

### Two defects the automated suite could not have caught

Both were found by this plan's own mandated live-browser step, and both are the
argument for keeping it.

**The cross-realm guard** (documented above under Phase 2) — all thirteen jsdom rows
then existing passed against a guard that returned `null` for every real click.

**The reveal-then-overwrite race.** `revealEditorLine` did not set `isSyncingRef`, and
by design it never takes focus — so *both* of `syncPreviewToEditor`'s feedback-loop
guards were unarmed after a reveal, and any real preview scroll within 50 ms
overwrote the correct reveal with a scroll-ratio-derived position. Measured: Monaco
correctly at 149-189 (containing the clicked line 171) at t=1 ms; a genuine 6 px
preview scroll at t=6 ms as the rich-text toolbar mounted and reflowed the page;
Monaco silently at 174-211 by t=83 ms, with no second `pointerup`. **Not
callout-specific** — any post-click reflow can do this to any block. Fixed by
bracketing the reveal with the file's existing `isSyncingRef`/300 ms idiom; bound by
U2e (which also asserts the suppression is *time-bounded*, so the fix cannot degrade
into disabling ratio sync outright) and by T5.

T5's binding depends on a post-activation reflow whose mechanism is documented as not
fully understood, so T5 asserts as an explicit precondition that a preview `scroll`
event actually fired. **If T5 ever fails with a scroll count of 0, that is the
fixture precondition breaking, not a product regression** — re-derive the fixture, do
not delete the row.

### Row inventory as shipped

| Rows | Tier | Binds |
|---|---|---|
| U1a, U1b | jsdom | innermost-wins resolution; no located ancestor |
| U1c | jsdom | the active-region guard (fixture must nest the region in a located **non-section** wrapper) |
| U1d | jsdom | the `<section>` guard — **synthetic; cannot occur in production today** (correction 2) |
| U1e | jsdom | non-Element targets don't throw |
| U1f | jsdom | the **cross-realm** duck-typed narrowing (reverting it reddens only this row) |
| U1g, U1h | jsdom | foreign `fileId` rejected; same-file control |
| U2a-U2d | jsdom | reveal is scroll-only, not focus-gated, not debounced |
| U2e | jsdom | reveal survives a following preview scroll, and suppression is time-bounded |
| U3 | jsdom | the capture-phase registration tuple — **the only binding for capture** (correction 1) |
| U4 | jsdom | the pre-existing preview-scroll→editor ratio path |
| T1 | Chromium | characterization: Chromium delivers no `click` when the `pointerup` target was detached |
| T2 | Chromium | the real reveal, provably not satisfiable by ratio matching |
| T3 | Chromium | HTML-preview control (D3: that path is untouched) |
| T4 | Chromium | the active-region guard in the real app |
| T5 | Chromium | no reveal-then-overwrite, with a self-check that the reflow really happened |

### Phase 4 — included content (D5), sequenced after Phase 2

**Investigation (task-7, 2026-08-21) — finding, decision, and pinned rows.**
Full writeup: `.superpowers/sdd/2026-08-21-preview-click-to-editor-scroll/task-7-report.md`.

- [x] **§0 — correction to the premise above: today's behaviour is NOT
      inert, it is wrong.** `lineForClickTarget` parses `fileId` out of
      `data-loc` (`parseDataLoc`) but never reads it — it returns
      `loc.startLine` unconditionally, and no caller
      (`Q2PreviewIframe.tsx`'s `handlePointerUp`, `useScrollSync.ts`'s
      `revealEditorLine`) checks `fileId` either. A click on included
      content reveals the included file's own line number *as if it were a
      line in the currently open file* — a real, editable, but **wrong**
      line, not "nothing." This is a live defect in code Phase 2 already
      shipped, independent of D5, and is fixable with a pure TS change (no
      wire change): reject when `fileId !== 0` (the rendered document is
      always `FileId(0)` — confirmed via `ParseDocumentStage` always running
      before `IncludeExpansionStage`, and `IncludeExpansionStage` only
      allocating fresh ids for spliced-in files). **Flagging prominently per
      the brief — whether to fix this ahead of / independent of the D5
      heuristic below is Gordon's call.**
- [x] **Investigate** whether the include splice point is recoverable.
      Confirmed via full read of `include_expansion.rs`:
      `blocks.splice(i..i+1, children)` (line 412) drops the shortcode's own
      block with no trace; spliced blocks carry only the included file's
      `Original` `SourceInfo` (remapped `FileId`). Two routes assessed in
      full in the report:
      - **(a) producer-side.** The `SourceInfo::Generated{by, from}` +
        `Anchor{role: Invocation}` machinery already exists and is used
        elsewhere (revealjs transforms, programmatic config) — `"include"`
        is even a documented `By` kind already. But it's a *replacement*
        variant, and spliced nodes must keep their `Original` info (the
        stage's own heading-id dedup and cache invalidation key off
        `root_file_id()`). So (a) needs either a new field alongside the
        existing location or a per-node rewrite pass added to
        `IncludeExpansionStage` — a real schema change to a *published,
        externalized* crate (`quarto-source-map`) plus its JSON writer plus
        the TS hand-mirror. Nested includes need a multi-hop anchor
        chain-walk on top of that. Out of scope for a TS-only bugfix branch
        (Phase 3), and not obviously smaller even as its own project.
      - **(b) client-side.** `files[fileId].name` (the *resolved* path) →
        scan the current file's source text for a `{{< include … >}}` line
        whose raw path resolves (mirroring `resolve_include_target`'s
        leading-`/`-is-project-root rule) to the same name. No wire change;
        cost is duplicating that one path-resolution rule in TS.
- [x] **Decision: route (b).** Reasoning (full version in the report §2):
      Phase 3 already commits this branch to TS-only/no-wire-change for good
      reason; (a) is a schema-level decision with cross-crate blast radius,
      not a small patch, and even fully built still needs the same
      "resolve to the nearest current-file anchor" logic for nested includes
      that (b) needs anyway. (b)'s heuristic failure modes all degrade to
      **inert** — never to a wrong reveal — which is exactly the property
      that made D4's guards acceptable in Phase 2. Stated fallbacks (§1 of
      the report):
      - **File included twice in the current file** → ambiguous which
        invocation → **inert**.
      - **Nested include** (not named in the current file's own source at
        all) → **inert**. (A chain-walk to the nearest ancestor invocation
        that *is* in the current file is a plausible future enhancement,
        needing either route-(a)-style provenance or the render's
        file-inclusion graph wired to the client — explicitly deferred, not
        silently dropped.)
      - **Code-fence includes** (`{{< include app.py >}}` spliced into
        fence *text*) already work correctly by construction — the fence's
        own `data-loc` is unaffected by what's inside its text, so no
        fallback is needed there.
- [x] **Pinned rows** (full table + rationale in the report §3): `U1f`/`U1g`
      (jsdom, `lineForClickTarget` extended with a `currentFileId` param —
      foreign fileId → `null`, same-file fileId unaffected), `U5a`/`U5b`/`U5c`
      (the route-(b) scan resolver: single match resolves, duplicate match is
      ambiguous → `null`, nested include has no textual match → `null`), and
      `E5` (Playwright: click included content, assert the editor stays on
      the current file and reveals the include shortcode's line).
- [ ] **Follow-up implementation** (not done in this task — investigation
      only, per the brief): land the §0 bugfix (foreign `fileId` → `null`,
      unconditionally) and the route-(b) scan enhancement, in that order;
      write `U1f`/`U1g` RED before the bugfix, `U5a`–`c`/`E5` RED before the
      enhancement.

## Housekeeping
- [x] Point `claude-notes/plans/CURRENT.md` at this plan.
- [x] Replace the `**Plan:**` placeholder in `CLAUDE.local.md`.
