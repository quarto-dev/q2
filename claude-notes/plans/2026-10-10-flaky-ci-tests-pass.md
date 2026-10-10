---
title: 'Flaky CI tests: the doc-inventory binary-entry race, and a pass over the October harness specs (bd-c72wsugj)'
date: 2026-10-10
description: 'Root-causes the quarto-sync-client doc-inventory flake (a 100 ms sync-throttle window the test disconnects inside) with a deterministic repro, inventories the other tests flaking in CI, and sketches a bounded fix pass.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-c72wsugj
---

**Branch:** `braid/bd-c72wsugj-flaky-ci-tests-pass` (topic branch in the main checkout, based on `main` @ `3b3e8aeaf`; no worktree by request)
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The doc-inventory flake has a confirmed root cause with a deterministic repro (five of five runs), and the fix is small and test-local. The "larger pass" the user asked for is bounded: CI history over the last 80 Hub-Client E2E runs names exactly three Playwright specs that flake, and two of them share one mechanism in the PDF viewer swap.

## Issue context

bd-c72wsugj (bug, P2, filed 2026-08-26 by Gordon Woodhull, labels `ci`, `flaky-test`): `ts-packages/quarto-sync-client/src/doc-inventory.test.ts` > "reports index, text, and binary docs with states and heads" fails intermittently at step 12 of `cargo xtask verify`. The inventory comes back with the index and `main.qmd` but no `logo.png` entry. Passes in isolation. The description suspects a timeout derivation because of `TimeoutNegativeWarning` noise in the same log.

The same failure has been filed three more times by three other people, each after one hit under load and clean reruns:

| Strand | Filed | By | Context |
| --- | --- | --- | --- |
| bd-fuw5gcni | 2026-08-18 | Gordon Woodhull | verify, Rust-only change; second hit noted by Carlos 2026-09-19 |
| bd-c72wsugj | 2026-08-26 | Gordon Woodhull | verify, Rust-only change (this strand) |
| bd-5sbguner | 2026-09-19 | Andrew Holz | two hits on 2026-09-18, one standalone `npm test -w`; first runs after a fresh `npm install` |
| bd-tg3vnfxg | 2026-09-23 | shikokuchuo | verify on the samod 0.14 bump |

Four strands, one test, one diff. The investigation linked them as `related`; whether to close three as duplicates is a question below.

## Dependency graph

- **discovered-from** bd-listing-inline-records-order-eq8n2usm (closed): a Rust-only listing change whose verify went red on this test. Confirms the flake is independent of the change under test.
- **related (incoming)** bd-c2yz067a (open): CLAUDE.md documents 5 verify steps while verify runs 14. Only connected because this flake aborts verify at step 12 of 14.
- **related (incoming)** bd-f1dr7gs1 (closed): hub-mcp Phase 0 conformance harness, which promised "no new flake modes" and listed this strand as a known one.
- **related (added here)** bd-fuw5gcni, bd-5sbguner, bd-tg3vnfxg (the duplicates) and bd-ag1q1eho (open: "fix async-settle races found in the PR #734 audit", the existing home for the Playwright one-shot-read sweep).

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

Every flaking spec but one was added in the 2026-10-01 to 2026-10-05 pandoc/typst host batch (28 of the 31 harness specs date from those five days). `projects-home-dialogs` is from 2026-08-28.

## What the code looks like today

### doc-inventory: confirmed root cause

Not a timeout derivation and not inventory logic. It is a flush race in the test helper, and the repro under `flaky-ci-tests-pass-investigation/` triggers it on every run.

1. automerge-repo 2.6.0-alpha.5 pushes local changes to peers from a trailing-edge `asyncThrottle` on each handle's `change` event (`DocSynchronizer.syncDebounceRate`, 100 ms). A change is sent no sooner than 100 ms after the previous flush of that doc.
2. `createNewProject` creates the index doc, then `main.qmd` (doc plus index change), then `await computeSHA256(...)`, then `logo.png` (doc plus index change). That `await` is the only yield between the two index changes.
3. When the yield outlasts the remaining throttle window (true under the CPU load of a full verify), the index doc flushes with only `main.qmd`, and the `logo.png` index entry lands in a second window 100 ms later.
4. The test helper then checks `hub.hubHasDoc(id)` for the index and both files. `hubHasDoc` is a presence check (`handle.doc() !== undefined`), satisfied by the partial index already on the hub. The `logo.png` doc itself arrives through the request path (the hub asks the creator for it), which is not throttled.
5. `creator.disconnect()` runs with the default `drainMs: 0` and closes the socket before the second window fires. The hub keeps an index with one file. The reader then loads exactly that: index plus `main.qmd`.

The repro delays `crypto.subtle.digest` by 150 ms (case A, fails with the CI diff) and shows the same schedule passing when the creator disconnects with `drainMs: 5000` (case B). `drainOutbound` already exists in `client.ts` and waits on `remote-heads` until a storage peer confirms the local heads; it just is not used here.

The `TimeoutNegativeWarning` lines in the strand are the throttle computing `lastCall + delay - Date.now()` as a negative wait. The copy of automerge-repo now in `node_modules` clamps it to 0 ("passing a negative delay to setTimeout warns on some runtimes"). Harmless then, gone now.

The same helper is copy-pasted in `sync-diagnostics.test.ts`, `dangling-entries.test.ts` (both packages), `offline-creation.test.ts` and `network-wrapper.test.ts`, and `hub-client/e2e/helpers/projectFactory.ts` has the browser version (`waitForServerDocuments` polls an HTTP presence endpoint, then `client.disconnect()`). Those create text files only, so there is no `await` between index changes and the hub's request for the index returns the full state; they are safe by accident. The `projectFactory` comment that documents "flush synchronously in online mode" is wrong about the library.

### PDF viewer: two failure shapes, one swap

`hub-client/src/pandoc/pdfViewer.ts` loads a recompile into a second iframe under the live one and promotes it (z-index 2, old frame removed) once `pagesloaded` and the first `pagerendered` have fired, or after `SWAP_TIMEOUT_MS` (5 s), whichever is first. The spec runs a `requestAnimationFrame` loop that counts frames in which the top iframe has no `.page canvas`, and asserts 0.

The Playwright error-context snapshots from PR #816 show two different end states:

- **Received 42** (retry): page 7 at 150% is selected, all twelve page regions exist and all are empty, the annotation-editor buttons are disabled. That is a viewer that was promoted before it had painted anything, which is what the 5 s timeout fallback produces. In the single-worker WebKit step, after eight minutes of pandoc and typst specs, a compile takes 5 to 10 s (the memory probe logs `ms: 9770`), so a pdf.js boot past 5 s is plausible.
- **Received 2** (first attempt): the final state is fine, page 1 painted at 150%. Two empty frames after promotion is the shape of a re-layout after the swap, for example the history restore applying scale 1.5 and page 7 after a first render at another scale, which makes pdf.js reset every page and redraw. The promotion condition is "any page rendered", not "the restored view rendered".

Both need a local WebKit reproduction under load before a fix is chosen; neither is resolvable by reading alone.

### pandoc-warm typing hook: likely the same swap

`test-hooks.ts` `typing()` returns once every run has ended plus one debounce, and counts `shown` trace events as frames. A frame is `shown` when the pane's `viewer.show` resolves, which is after the iframe swap, which can lag the run end by up to the 5 s swap timeout in a slow WebKit. One failure (frames 1, expected 2) matches the hook returning between the run end and the frame. Same fix family as the viewer.

### projects-home-dialogs: one-shot flake, old spec

Menu visible, first item focused, Enter pressed, then `[role=dialog]` never appears within 5 s; passed on retry. One hit in the window. It is the kind of race bd-ag1q1eho already covers (press before the handler is attached, or focus assertion passing on a stale node). Worth a look in the same pass but not worth a design discussion.

## Proposed phases (draft)

- Phase 0: turn the repro into a regression test in `quarto-sync-client` (the digest delay makes it deterministic and cheap). Decide its shape with question 2.
- Phase 1: doc-inventory. One shared test helper (probably in `test-hub.ts`) that creates a project and waits until the hub holds the creator's *heads*, not just the doc, and/or disconnects the creator with `drainMs`. Replace the six copies. Fix the `projectFactory` comment and consider the same drain there (bd-3nzyd territory). Close the duplicate strands.
- Phase 2: PDF viewer swap. Reproduce the 42-frame and 2-frame shapes in WebKit under CPU load locally. Then choose between: promote on the restored view (first `pagerendered` after `setInitialView`), a WebKit-aware or load-aware swap timeout, and making the spec assert the no-empty-frame invariant only when the swap did not fall back to the timeout. Re-check `typing()` against the same change.
- Phase 3: `projects-home-dialogs` plus a grep-driven audit of the 2026-10 harness batch for one-shot reads on async state (`expect(await page.evaluate(...))`), per `.claude/rules/hub-client-tests.md`. File findings on bd-ag1q1eho rather than fix them all here.
- Phase 4: CI policy. The harness config comment says "assertion failures are deterministic and still fail through the retry", which the timing assertions above have made untrue; the WebKit step is gating with one retry. Decide per question 5.
- Phase 5: docs. Add the "presence is not delivery" lesson for sync tests to `.claude/rules/hub-client-tests.md` or a sibling rule, and record the throttle semantics somewhere findable.

## Open design questions for the user

1. **Where the doc-inventory fix lives.** Test-side only (shared helper: heads-aware wait plus drained disconnect), or also make `createNewProject` drain its own outbound sync before resolving when it is online? The library change would also cover `projectFactory` and any future caller, at the cost of up to one throttle window plus a round trip on every online project creation in hub-client. Recommendation: test-side now, library change as its own strand if wanted.
2. **Keep the digest-delay repro as a permanent test?** It mocks `crypto.subtle.digest` globally for one file. It is the only way to make this schedule deterministic without a load generator. Recommendation: yes, next to `exit-drain.test.ts`.
3. **Scope of the harness pass.** The three specs with CI evidence, or all 28 specs from the 2026-10-01 to 10-05 batch? Recommendation: fix the three, audit the rest by grep, and file anything found on bd-ag1q1eho.
4. **Is "never an empty frame" a product invariant or best effort?** The 5 s swap timeout deliberately shows an unpainted viewer rather than wait forever, so the current product code violates the spec's assertion by design under load. Either the timeout fallback is acceptable and the spec must tolerate it, or it is not and the viewer needs a different fallback (keep the old frame on top until the new one paints, with a longer or no timeout).
5. **WebKit gating.** Two of the last 40 `main` E2E runs went red on this one WebKit test. Keep WebKit gating with one retry while Phase 2 lands, or make it non-gating like Firefox until then?
6. **Duplicate strands.** Close bd-fuw5gcni, bd-5sbguner and bd-tg3vnfxg now as duplicates of bd-c72wsugj, or when the fix merges?

## Risks / tradeoffs (draft)

- A heads-aware `hubHasDoc` depends on automerge-repo internals (`getSyncInfo`, `remote-heads`) that `drainOutbound` already relies on; a future automerge-repo bump can break both at once. The existing `exit-drain.test.ts` is the canary.
- Raising the swap timeout trades a flash for a longer stale view under load; lowering it trades the other way. The right answer depends on question 4, and the spec cannot assert a timing invariant the product does not promise.
- The grep audit in Phase 3 can balloon. Keep it to findings filed on bd-ag1q1eho, not fixes, unless they are one-liners.
- `cargo xtask verify` ran cold in this checkout during the investigation; its result is recorded in the hand-back, not here.
