---
title: 'Flaky CI tests: the doc-inventory binary-entry race, and a pass over the October harness specs (bd-c72wsugj)'
date: 2026-10-10
description: 'Root-causes the quarto-sync-client doc-inventory flake (a 100 ms sync-throttle window the test disconnects inside) with a deterministic repro, inventories the other tests flaking in CI, and fixes the three with CI evidence.'
status: in-progress
braid:
  strand: bd-c72wsugj
---

**Branch:** `braid/bd-c72wsugj-flaky-ci-tests-pass` (topic branch in the main checkout, based on `main` @ `3b3e8aeaf`; no worktree by request)

## Triage verdict

**Ready to design** (2026-10-10, morning). The doc-inventory flake has a confirmed root cause with a deterministic repro (five of five runs), and the fix is small and test-local. The "larger pass" the user asked for is bounded: CI history over the last 80 Hub-Client E2E runs names exactly three Playwright specs that flake.

**Decided and implemented** (2026-10-10, afternoon; see "Decisions" below). One question stays open: whether the viewer's swap timeout is acceptable product behavior.

## Decisions

The user answered the six questions the skeleton asked (quoted under "Open design questions" below, kept for the record):

1. Doc-inventory fix is test-side only. Done: `TestHub.hubHasHeads` / `hubHasHeadsOf` in `test-hub.ts`; every project-creating helper in `quarto-sync-client` waits for the creator's heads on the hub before the creator disconnects; the hub-client E2E project factory drains its disconnect and its stale "flush synchronously" comment is corrected.
2. The digest-delay repro is a permanent test: `ts-packages/quarto-sync-client/src/project-creation-delivery.test.ts`.
3. Fix the three specs with CI evidence; audit the rest by grep and file findings on bd-ag1q1eho. Done; findings are in that strand's notes. The code is organized for bd-ag1q1eho: the "wait on the state" patterns are documented in `.claude/rules/hub-client-tests.md` (now also scoped to the sync-client and hub-mcp tests), the typing hook waits on trace events rather than a debounce, and the Menu primitive keeps focus when its items change.
4. Open. See "The swap timeout" below.
5. WebKit stays gating with one retry. No CI change in this pass.
6. The three duplicate strands (bd-fuw5gcni, bd-5sbguner, bd-tg3vnfxg) were closed on 2026-10-10 as duplicates of bd-c72wsugj.

## Issue context

bd-c72wsugj (bug, P2, filed 2026-08-26 by Gordon Woodhull, labels `ci`, `flaky-test`): `ts-packages/quarto-sync-client/src/doc-inventory.test.ts` > "reports index, text, and binary docs with states and heads" fails intermittently at step 12 of `cargo xtask verify`. The inventory comes back with the index and `main.qmd` but no `logo.png` entry. Passes in isolation. The description suspects a timeout derivation because of `TimeoutNegativeWarning` noise in the same log.

The same failure had been filed three more times by three other people, each after one hit under load and clean reruns:

| Strand | Filed | By | Context |
| --- | --- | --- | --- |
| bd-fuw5gcni | 2026-08-18 | Gordon Woodhull | verify, Rust-only change; second hit noted by Carlos 2026-09-19 |
| bd-c72wsugj | 2026-08-26 | Gordon Woodhull | verify, Rust-only change (this strand) |
| bd-5sbguner | 2026-09-19 | Andrew Holz | two hits on 2026-09-18, one standalone `npm test -w`; first runs after a fresh `npm install` |
| bd-tg3vnfxg | 2026-09-23 | shikokuchuo | verify on the samod 0.14 bump |

Four strands, one test, one diff. The three others are closed as duplicates.

## Dependency graph

- **discovered-from** bd-listing-inline-records-order-eq8n2usm (closed): a Rust-only listing change whose verify went red on this test. Confirms the flake is independent of the change under test.
- **related (incoming)** bd-c2yz067a (open): CLAUDE.md documents 5 verify steps while verify runs 14. Only connected because this flake aborts verify at step 12 of 14.
- **related (incoming)** bd-f1dr7gs1 (closed): hub-mcp Phase 0 conformance harness, which promised "no new flake modes" and listed this strand as a known one.
- **related (added here)** bd-fuw5gcni, bd-5sbguner, bd-tg3vnfxg (the duplicates, now closed) and bd-ag1q1eho (open: "fix async-settle races found in the PR #734 audit", the existing home for the Playwright one-shot-read sweep; this pass filed its audit findings there).

No blockers in either direction.

## What is actually flaking in CI

The user pointed at PR #816 as an example. Its red check is the Hub-Client E2E chromium job, and the failing step is "Run pandoc harness tests in WebKit", not the doc-inventory test. (The earlier red Test Suite run on that PR was a deterministic Q-2-7 apostrophe error in a plan file, fixed in the next commit.)

Failing Playwright specs across the last 80 Hub-Client E2E runs on all branches, and the last 40 on `main`:

| Spec and test | Project | Hits | Where |
| --- | --- | --- | --- |
| `pandoc-pdf-viewer.harness.spec.ts` > "the preview pane: ... keeps the reader where they were" | webkit (hard fail after retry); once also chromium | 4 | main 2026-10-07, main 2026-10-08, bd-3qe7unp7 branch 2026-10-07, PR #816 2026-10-10 |
| `pandoc-warm.harness.spec.ts` > "the typing hook drives the real pane and logs frames..." | webkit | 1 | bd-rpra6kpq branch 2026-10-07 |
| `projects-home-dialogs.harness.spec.ts` > "New project dialog opens via the New menu..." | chromium (passed on retry) | 1 | PR #816 2026-10-10 |
| `q2-sandboxed-preview.spec.ts` > "renders a themed document with math..." | chromium and firefox, both runs | 2 | snyk-upgrade branch only; deterministic there, so a Snyk-bump regression, not a flake. Out of scope. |

The four other red `main` runs in the window (2026-10-02 and 2026-10-03) failed in "Build TypeScript packages", and the two red TS Test Suite runs on `main` failed in "Lint hub-client CSS". Neither is a test flake. The doc-inventory test has not failed in CI in this window; its hits are all local `cargo xtask verify` runs.

Every flaking spec but one was added in the 2026-10-01 to 2026-10-05 pandoc/typst host batch (19 harness specs date from those five days). `projects-home-dialogs` is from 2026-08-28.

## What the code looked like, and what changed

### doc-inventory: confirmed root cause, fixed test-side

Not a timeout derivation and not inventory logic. It is a flush race in the test helper, and the repro under `flaky-ci-tests-pass-investigation/` triggers it on every run.

1. automerge-repo 2.6.0-alpha.5 pushes local changes to peers from a trailing-edge `asyncThrottle` on each handle's `change` event (`DocSynchronizer.syncDebounceRate`, 100 ms). A change is sent no sooner than 100 ms after the previous flush of that doc.
2. `createNewProject` creates the index doc, then `main.qmd` (doc plus index change), then `await computeSHA256(...)`, then `logo.png` (doc plus index change). That `await` is the only yield between the two index changes.
3. When the yield outlasts the remaining throttle window (true under the CPU load of a full verify), the index doc flushes with only `main.qmd`, and the `logo.png` index entry lands in a second window 100 ms later.
4. The test helper then checked `hub.hubHasDoc(id)` for the index and both files. `hubHasDoc` is a presence check (`handle.doc() !== undefined`), satisfied by the partial index already on the hub. The `logo.png` doc itself arrives through the request path (the hub asks the creator for it), which is not throttled.
5. `creator.disconnect()` ran with the default `drainMs: 0` and closed the socket before the second window fired. The hub kept an index with one file. The reader then loaded exactly that: index plus `main.qmd`.

The repro delays `crypto.subtle.digest` by 150 ms (case A, fails with the CI diff) and shows the same schedule passing when the creator disconnects with `drainMs: 5000` (case B). The `TimeoutNegativeWarning` lines in the strand are the throttle computing `lastCall + delay - Date.now()` as a negative wait; the copy of automerge-repo now in `node_modules` clamps it to 0. Harmless then, gone now.

**Fix.** `TestHub` gained `hubHasHeads(docId, heads)` (the hub holds every change up to the given heads, checked by change hash) and `hubHasHeadsOf(client)` (the same for every doc in the client's inventory). The seven project-creating helpers across `doc-inventory`, `dangling-entries`, `sync-diagnostics`, `network-wrapper`, `author-id` and `offline-creation` now wait on that before the creator disconnects; `hubHasDoc` keeps its presence semantics (the exit-drain tests need it after the client is gone) and documents the difference. `project-creation-delivery.test.ts` forces the slow-digest schedule against the fixed helper; reverting the helper to `hubHasDoc` fails it deterministically. The hub-client E2E `createProjectOnServer` drains its disconnect (`drainMs: 5000`), and its comment no longer claims that online creation flushes synchronously.

### PDF viewer: the promotion gate accepted a draw that pdf.js was about to throw away

`hub-client/src/pandoc/pdfViewer.ts` loads a recompile into a second iframe under the live one and promotes it (z-index 2, old frame removed) once the new viewer has painted. The gate was `pagesloaded` plus the first `pagerendered`, or `SWAP_TIMEOUT_MS` (5 s), whichever came first. The spec runs a `requestAnimationFrame` loop that counts frames in which the top iframe has no `.page canvas`, and asserts 0.

Reading `public/pdfjs/web/viewer.mjs` with the Playwright error-context snapshots from PR #816 (page 7 selected at 150%, page 1 the only page with content; or every page region empty) gives one mechanism for both shapes:

- `PDFViewer.setDocument` creates the page views, dispatches `pagesinit`, and immediately calls `update()`, which draws the first page at the default zoom.
- `PDFViewerApplication.load` applies the saved zoom and page only after `storedPromise` and three worker round trips (`getPageLayout`, `getPageMode`, `getOpenAction`) resolve: `setInitialView(hash)` sets `isInitialViewSet`, changes the scale, resets every page view (canvases removed, in-flight renders cancelled) and scrolls to page 7, then dispatches `documentinit`.
- On a fast machine the second step wins and the first draw is cancelled before it finishes; the first `pagerendered` is page 7 at 150%. Under load the first draw finishes first: `pagerendered` (page 1, default zoom) and `pagesloaded` (twelve pages, cheap) fire, the host promotes the new frame, and then `setInitialView` empties it. Two empty frames when the redraw is quick; 42 when the 150% redraw in WebKit is slow.

The local WebKit run of the spec takes 3.5 s per test; the failing CI runs took 10 to 14 s, so a 3 to 4x slower environment is enough to flip the race. The 5 s timeout is not needed to explain either snapshot.

**Fix.** The gate is now: `pagesloaded`, `isInitialViewSet` true, and the current page's view in `RenderingStates.FINISHED`, re-checked on `documentinit` and on every `pagerendered`. A render that completes after `setInitialView` was started after the reset (the reset cancels in-flight renders, and cancelled renders dispatch no `pagerendered`), so it is at the final layout. `pdfViewer.test.ts` drives a fake `PDFViewerApplication` through both orders (draw before the initial view, then reset, then draw; and draw that survives the initial view). The 5 s timeout is unchanged pending question 4.

### pandoc-warm typing hook: a fixed sleep, not the swap

The skeleton guessed this was the viewer swap. It is not: `DownloadController` emits the `shown` trace event before `onPdf` hands the bytes to the viewer and before `end`, so a run's frame is always logged before the run ends. The hook's problem was its ending: after the last edit it slept one debounce plus 50 ms, checked `live === 0`, and repeated once. The first frame's `shown` fires while the viewer iframe is still booting pdf.js on the same thread; that boot delays the React commit of the edit and the pane's 100 ms debounce timer, so the edit's run can start after both checks. The hook then returned with one frame and one start, which is the CI failure (frames 1, expected 2).

**Fix.** `typing()` waits for a run start at or after the last edit's timestamp (the trailing-edge debounce always produces one), then for every run to end. No sleeps.

### projects-home-dialogs: the menu's items changed under the focused item

The New menu renders a placeholder item (`id: 'default'`) until `getProjectChoices()` resolves, which awaits the WASM module. The registry's choices all live in groups (`Templates`, `Examples`), so when they arrive the placeholder unmounts and two submenu parents take its place. Chromium fires no blur when a focused element is removed; focus falls to `<body>`, and the Enter the spec presses next reaches nothing. The spec passed whenever the WASM load (1 to 3 s in CI) outlasted its first few steps, and asserted on the placeholder, never on the real menu.

**Fix.** `Menu` keeps keyboard focus: the root's focus and blur events record whether an item of the menu holds focus, and a layout effect after every render refocuses the first item when it did and nothing holds it now (focus that moved to another element, such as the trigger on close, is left alone). A first version recorded that only inside the layout effect, which never runs between the mount-time focus and the item swap; the load run caught it (three of three failures), and the event-based version passed. The spec waits for the real menu (the `Templates` group focused), opens it with ArrowRight, and activates `Default` with Enter, so it exercises the menu users get. While there, the five pre-existing `react-hooks/refs` lint errors in `Menu.tsx` (the focus-return target was read from refs during render) were fixed by capturing it in a mount-time layout effect.

### The swap timeout (question 4, still open)

The viewer's header comment states the promise: a recompile replaces the live PDF "in one paint", only once the new viewer has drawn, because reopening in place "empties it for a few frames, a visible flash". The spec asserts exactly that promise: zero frames with an unpainted viewer on top.

`SWAP_TIMEOUT_MS` is the one place the product chooses to break it: if the new viewer has not painted 5 s after pdf.js initialized, it is promoted anyway, blank, and the reader watches it fill in. The comment calls this "slower than that, show it anyway". It exists for a viewer that never paints (a pdf.js failure the host would otherwise wait on forever), but it fires on slowness, not failure, and a loaded CI runner is slow: the memory probe measured a 9.8 s compile in one of the failing runs. So the question is which promise the product makes:

- **An invariant.** The old PDF stays on top until the new one has painted, however long that takes; a viewer that never paints is an error (reject `ready`, keep the old PDF, show the pane's error banner, with a long watchdog of the order of 30 s so a dead viewer still surfaces). The spec then asserts the product's promise and is deterministic in principle.
- **Best effort.** A blank viewer after 5 s is acceptable. Then the spec cannot assert 0 unconditionally: it must either be told that the swap fell back and only assert when it did not, or disable the timeout through a test hook.

Recommendation: the invariant. Promoting an unpainted viewer is the exact flash the second iframe exists to prevent, and the one case the timeout helps is better shown as an error than as a blank pane. Until that is decided, the residual CI risk on this spec is a swap slower than 5 s, which the local evidence (3.5 s for the whole test, two compiles and two swaps included) puts well below the typical run, but not out of reach of a heavily loaded runner.

## Verification

- `quarto-sync-client`: the eight touched test files plus `exit-drain` pass (26 tests). `project-creation-delivery.test.ts` fails deterministically with the helper reverted to `hubHasDoc`.
- `hub-client`: `pdfViewer.test.ts` (4), `Menu.submenu.integration.test.tsx` (8), `ProjectsHome.newMenu.integration.test.tsx` (4) pass; ESLint is clean on every touched file.
- Harness specs, local, `--retries=0 --workers=1`: `projects-home-dialogs` (chromium) 3 of 3, the pandoc-warm typing-hook test (webkit) 3 of 3, `pandoc-pdf-viewer` (webkit) 6 of 6 at normal load.
- Under CPU load (36 busy loops on an 18-core machine): dialogs 8 of 8 (the first Menu fix failed 3 of 3 here, see above), typing hook 3 of 3, viewer 6 of 6.
- The viewer race did not reproduce locally against the old gate: 4 of 4 passes under the same load, and 6 of 6 with Chromium CPU throttling at 8x and 16x applied from the recompile on. Uniform slowdown does not flip the order of pdf.js's first draw and its stored-view read here, so the gate fix rests on the pdf.js source, the two CI snapshots, and the unit test that drives both orders through a fake viewer. CI is the real check for this one.

## Proposed phases (as executed)

- Phase 0: the repro became `project-creation-delivery.test.ts`; the viewer gate got a unit test first.
- Phase 1: doc-inventory, test-side (above).
- Phase 2: PDF viewer gate; typing hook; Menu focus and the dialogs spec.
- Phase 3: grep audit of the October batch, filed on bd-ag1q1eho.
- Phase 4: CI policy unchanged (question 5).
- Phase 5: `.claude/rules/hub-client-tests.md` gained the sync "presence is not delivery" section and two Playwright bullets, and now applies to the sync-client and hub-mcp test files too.

## Open design questions for the user

Asked on 2026-10-10; answers recorded under "Decisions".

1. **Where the doc-inventory fix lives.** Test-side only (shared helper: heads-aware wait plus drained disconnect), or also make `createNewProject` drain its own outbound sync before resolving when it is online? The library change would also cover `projectFactory` and any future caller, at the cost of up to one throttle window plus a round trip on every online project creation in hub-client. Recommendation: test-side now, library change as its own strand if wanted.
2. **Keep the digest-delay repro as a permanent test?** It mocks `crypto.subtle.digest` globally for one file. It is the only way to make this schedule deterministic without a load generator. Recommendation: yes, next to `exit-drain.test.ts`.
3. **Scope of the harness pass.** The three specs with CI evidence, or all specs from the 2026-10-01 to 10-05 batch? Recommendation: fix the three, audit the rest by grep, and file anything found on bd-ag1q1eho.
4. **Is "never an empty frame" a product invariant or best effort?** The 5 s swap timeout deliberately shows an unpainted viewer rather than wait forever, so the current product code violates the spec's assertion by design under load. Either the timeout fallback is acceptable and the spec must tolerate it, or it is not and the viewer needs a different fallback (keep the old frame on top until the new one paints, with a longer or no timeout). Expanded above.
5. **WebKit gating.** Two of the last 40 `main` E2E runs went red on this one WebKit test. Keep WebKit gating with one retry while Phase 2 lands, or make it non-gating like Firefox until then?
6. **Duplicate strands.** Close bd-fuw5gcni, bd-5sbguner and bd-tg3vnfxg now as duplicates of bd-c72wsugj, or when the fix merges?

## Risks / tradeoffs

- `hubHasHeads` depends on automerge's change hashes matching `DocHandle.heads()` after `decodeHeads`; a future automerge-repo bump that changes the heads encoding breaks the helper loudly (every creator wait times out), which is the right failure.
- The viewer gate reads two pdf.js internals (`isInitialViewSet`, `getPageView(i).renderingState`). A pdf.js upgrade that renames them makes `painted()` false forever and every swap fall back to the timeout, which the spec would catch as empty frames.
- The Menu focus effect runs after every render of an open menu. It only acts when focus has fallen to `<body>`, so it cannot steal focus from a dialog or the trigger.
- The dialogs spec now needs the WASM registry to answer in the harness. If WASM init ever fails there, the spec times out waiting for `Templates` instead of passing against the placeholder, which is the honest outcome.
