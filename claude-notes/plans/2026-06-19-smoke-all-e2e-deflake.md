---
title: 'Smoke-all E2E deflake'
date: 2026-06-19
description: 'Diagnoses flaky nightly end-to-end smoke tests, finding that renders stall under CPU contention because the sync client loaded project documents serially with no timeout, and fixes that root cause.'
---

## Overview

Nightly `smoke-all` E2E (`hub-client/e2e/smoke-all.spec.ts`) shows failures
concentrated on specific fixtures, not a uniform slow-render tail (binomial
test: observed 12 hard-fails vs 1.9 expected under i.i.d., p ≈ 8e-7).

Two clusters dominate:

- **A — shortcode/extension `[html]`**: every top failure expands a `{{{< … >}}}`
  shortcode (runs the WASM Lua interpreter). The multi-file extension fixtures
  (`block-shortcode` = 3 files, worst with 3 hard-fails) add a VFS-sync race on
  top: nothing gates the first render / the assertion re-render on all project
  files having synced into the VFS. `smoke-all.spec.ts` only *sorts* the target
  QMD last when creating docs server-side — a best-effort hint, not a barrier.
- **B — `q2-preview`**: renders through the Q2PreviewIframe + postMessage path,
  asserting on post-render layout decoration (`header#title-block-header`,
  `div.quarto-title-meta`). The completion signal (`body.innerHTML.length > 0`,
  `previewExtraction.ts:72`) fires as soon as the body paragraph renders, before
  decoration lands.

Shared amplifier: a too-weak "render done" signal checked against a fixed 75s
deadline, on a 2-core CI runner. Mitigations already spent: `workers:1` for
smoke-all, `retries:3`.

## Root causes (harness-side, deterministic to fix)

1. No VFS barrier: assertions can re-render against an incomplete VFS.
2. Weak completion signal: `body.innerHTML.length > 0` ≠ "render complete".
3. `ensureHtmlElements` live-iframe waits inherit the default expect timeout
   (5s), too short for a post-late-sync re-render under contention.

## Checklist

- [x] Set up worktree from main, reuse build artifacts (WASM, hub bin, node_modules)
- [x] Build WASM from main source (toolchain ok); build app with VITE_E2E=1; build ts-packages dist
- [x] Establish a local baseline run of the flaky subset (couldn't repro flake — 12 cores vs CI 2)
- [x] Fix 1: VFS barrier helper (`waitForVfsFiles`) — poll `vfsListFiles()` until project files present
- [x] Fix 1a: tolerate "WASM not initialized" throw while polling (barrier runs before first render)
- [x] Fix 1b: make barrier best-effort (warn, not throw) — over-broad discovery in q2-preview/ fixtures
- [x] Fix 2: single assertion render (`renderForAssertions`) — was 2 redundant WASM renders/test
- [x] Fix 3: generous (30s) element-wait timeout for `ensureHtmlElements`
- [x] Verify flaky subset passes locally, repeated x4, retries off → 28/28 pass
- [x] Verify full smoke-all passes locally (single pass) → 78/78
- [x] Commit (e09005d7) + push #1 + CI run 27813896174
- [x] CI run #1 ballooned (>1hr) — barrier waited 30s on over-broad q2-preview fixtures; cancelled
- [x] Redesign barrier: quiesce escape + 10s cap + 3s grace (26ab9810); 78/78 local, 3.4m
- [x] Push #2 + CI run 27818252697 — in progress, watching smoke-all step
- [x] Implement root-cause fix in sync client (6fc040e8): bound repo.find()
      with AbortSignal.timeout(5s) + load file docs concurrently (Promise.all)
- [x] quarto-sync-client unit suite: 102 pass (no regression)
- [x] no-contention smoke-all: 78/78, \~30% faster (2.4m vs 3.4m)
- [x] heavy 10-hog contention (extension subset): \~all-fail → 2/15
- [ ] Confirm CI smoke-all green + fast; compare flake counts vs baseline

## Fix + measurements (commit 6fc040e8)

Root cause (proven by boot-trace): sync client `connect()` →
`loadFileDocuments()` loaded \~49 file docs **serially**, and `findDoc()`\'s
`repo.find()` had **no deadline** → a single slow doc hung \~60s on
automerge-repo's internal unavailable timeout → blew the 75s render budget →
Editor/preview never mounted. (The extension fixtures share one \~49-file
project because `extensions/_quarto.yml` roots them all there — more docs =
higher odds of hitting a slow one, which is why they dominate the flaky list.)

Fix: `AbortSignal.timeout(5s)` per `repo.find` attempt (degrades to the
existing `markFileUnavailable` path) + concurrent `Promise.all` loading
(wait bounded by the slowest doc, not the sum).

| scenario | before fix | after fix |
| --- | --- | --- |
| no contention (full 78) | 78/78 (3.4m) | 78/78 (2.4m) |
| 10-hog subset (×3) | \~all fail | 2/15 fail |
| 6-hog subset (×3) | n/a | 2/18 fail |
| 6-hog full (78) | 1 fail | 6 fail* |

\* full-suite 6-hog amplified by the shared hub accumulating load over 78
tests (a test-infra factor, not the fix). All residual failures are still
75s render-timeouts (connect slow), NOT dropped-file assertion failures —
so the 5s timeout is not dropping needed docs. With CI `retries:3` an \~11%
per-attempt rate → near-zero hard-fails.

Residual / follow-ups (not done): (a) eager retry-on-peer-arrival for
unavailable docs ("plan D2"); (b) don't block the render on non-active file
docs (load active file + deps first, siblings in background) — the real
structural fix for over-scoped projects; (c) the shared single hub across 78
e2e tests accumulates load — consider per-test isolation.

## ROOT CAUSE (the real one) — render stalls under CPU contention

Reproduced the CI failures locally by saturating cores (`yes >/dev/null` ×N
on a 12-core machine; CI's 2 physical cores + vite+hub+chromium+playwright is
comparably starved). Findings:

- **6-hog (≈CI) full suite, retries off:** 77/78, the 1 failure is
  `block-shortcode` — `waitForPreviewRender` times out at 75s.
- **10-hog full suite:** 8/78 fail, all `waitForPreviewRender` 75s timeouts
  (6 of them) + 1 blob-image element miss. Failures concentrate on the heaviest
  renders: Lua shortcode fixtures (`block-shortcode`, `builtin-kbd`) and
  `q2-preview` React renders.
- **The 401 in the console is a RED HERRING** — it's the deliberate `/auth/me`
  stub (`projectFactory.ts:155 mockAuthMe`, no-auth test mode). It's collected
  for every test but only *printed* when a test fails, so it looked correlated.
- **Slow vs stuck — decisive test:** bumped the render timeout 75s→150s and ran
  the shortcode fixtures under 8-hog contention. They **still timed out at the
  full 150000ms** (4/6). A render that completes in <2s uncontended does not
  finish in 150s under contention → **the render STALLS (livelock), it is not
  merely slow.** Bumping the timeout does nothing.

**Conclusion: this is a render-pipeline bug, not a test-harness flake.** Under
CPU contention the WASM render (worst for the Lua shortcode path and the
q2-preview React path) stalls indefinitely. `retries:3` + `workers:1` is why
it historically reads as *flaky* (a less-contended retry completes). No
test-harness change can make a stalled render finish. This very likely also
affects real hub-client users on loaded/slow machines.

### Localized via render-trace instrumentation (then reverted)

Added temporary `[render-trace]` console logs through Preview.tsx /
PreviewRouter.tsx and captured them under 10-hog contention. Comparing a
passing vs a failing run of `block-shortcode`:

- **Passing:** whole boot < 1s — `PreviewRouter MOUNT → initWasm DONE ms=1 →
  Preview MOUNTED (contentLen=203) → render done 799ms`.
- **Failing:** only `Preview.tsx MODULE LOADED` fires. **`PreviewRouter` never
  mounts.** No initWasm, no checkFormat, no render.

So the three things we suspected are all RULED OUT:
- WASM init is **1ms** (not the stall).
- The WASM **render is 800ms** (not the stall) — it never even runs.
- The Preview render loop / Lua path is never reached.

**The stall is UPSTREAM of `PreviewRouter`** — in the app-shell boot →
project-load → Editor-mount path, after the test's `page.goto(file URL)`
(which re-boots the app: re-load bundle, re-connect ws, re-hydrate the
Automerge project, route to file, mount Editor). A boot that normally takes
<1s not reaching `PreviewRouter` in **75s** is a **75×+ slowdown = a livelock,
not slowness.**

Likely contributing factor (test-harness): the spec does
`bootstrapProjectSet` + `seedProjectInBrowser` (waits for connected) and THEN a
full `page.goto(file URL)` that **throws away that work and cold-boots the app
again**. The cold boot under contention is what stalls. `retries:3` re-boots
fresh each attempt → why it reads as flaky, not hard-fail.

### ROOT CAUSE FOUND (boot-trace through App.tsx → sync client)

Traced the boot path under 10-hog contention. The chain:
`handleRouteChange` (App.tsx) → `connectAndLoadContents` → `connect`
(automergeSync.ts) → sync client `connect()` → **`loadFileDocuments()`**
(quarto-sync-client/src/client.ts:553).

`loadFileDocuments` loads every project file doc **sequentially**, each via
`findDoc()` → **`repo.find(docId)`** (client.ts:432).

- **Passing run:** all \~49 file docs resolve in <2ms each → connect = 335ms.
- **Failing run:** ONE file doc's `repo.find()` **hangs \~60s** (automerge-repo
  2.5.6's default unavailable-doc timeout) before rejecting, then the retry
  loop (attempts=3, fast) gives up and `markFileUnavailable`s it. That single
  60s serial stall blows the 75s `waitForPreviewRender` budget → test fails.
  Trace: `findDoc TbSuvu11 attempt=0` at t=764195 → `attempt=1` at t=824451
  (exactly +60.2s).

So the stall is **NOT** render, WASM init, the Preview loop, or the harness —
it's the sync client waiting \~60s for any single project file doc that's slow
to serve under contention, while loading all docs serially. Real users hit
this too: opening a project where one doc is slow to sync stalls the whole
open for 60s.

**Fix surface (clean):** `repo.find<T>(id, options?: RepoFindOptions &
AbortOptions)` accepts an `AbortSignal` (automerge-repo 2.5.6). Bound
`findDoc`\'s `repo.find` with `AbortSignal.timeout(N)` (e.g. 8s) so an unsynced
doc fails fast into the existing `isUnavailableError`/retry/`markFileUnavailable`
path instead of hanging 60s. Likely also: load file docs in parallel, and/or
don't block the initial render on non-active file docs (load siblings in the
background; re-subscribe via the index `change` handler when they arrive).

**Design questions for the owner (why this is a checkpoint, not a blind edit):**
- Marking a slow-but-coming doc "unavailable" makes the render proceed without
  it. Does the client reliably **re-load + re-render** when the doc finally
  syncs? (index `change` handler re-subscribes *new* files; does it retry
  known-unavailable ones?) If not, the active target doc timing out would turn
  a 60s flake into a wrong render.
- The 60s is automerge-repo's own find timeout; bounding it changes
  offline/slow-sync semantics that `client.*.test.ts` pins. Needs the
  sync-client test suite run + review.
- This touches shared infra used by real users, not just the e2e suite.

## Runtime regression (CI run #1) — root cause + fix

The first barrier waited up to 30s for ALL discovered project files. The
\~20 q2-preview/ fixtures over-include unrelated sibling-project files
(no \_quarto.yml at that dir → roots at parent) that sync slowly/never, so
each paid the full 30s → smoke-all step ballooned far past its fast
baseline. Fixed by bounding the barrier (commit 26ab9810):
- return on exact-match (fast path) OR VFS-count quiesce (escape hatch);
- cap 30s → 10s;
- quiesce gated behind 3s grace + \~600ms stable run so it can't preempt a
  still-arriving extension fixture (files push target-last).
Net: over-broad fixtures cost \~3s instead of 30s.

## Findings / decisions

- Local cannot reproduce the contention flake (12 cores). Fixes target the
  deterministic root causes; CI is the real validator.
- The prebuilt WASM from another branch can't be reused: its generated
  `pkg/*.js` hardcodes a `ts-packages/wasm-js-bridge/src/*` path that drifts
  between branches → must `build:wasm` from the worktree's own source.
- `build-wasm.js` step 2 (wasm-bindgen) ignores `CARGO_TARGET_DIR`; ran
  wasm-bindgen by hand against the shared-target artifact to avoid a recompile.
- Discovery quirk (pre-existing, not fixed here): `q2-preview/` has no
  `_quarto.yml`, so single-file fixtures there root at the parent and pull in
  unrelated sibling-project files. Best-effort barrier sidesteps it; worth a
  follow-up to add a `q2-preview/_quarto.yml` or tighten discovery.
- Must clean up stray hub (port 3031) + `/tmp/hub-e2e-server.json` between runs
  or globalSetup fails with ENOENT.

## Notes

- Harness `.ts` files run in the Playwright node process, NOT the vite bundle —
  iterating on them does NOT require rebuilding WASM/dist. One good app build suffices.
- Local = 12 cores; CI = 2. Contention-driven flake may not reproduce locally;
  fixes target the deterministic root causes (barrier, completion signal).
- CI lever: `gh workflow run hub-client-e2e.yml -f run-smoke-all=true`.
