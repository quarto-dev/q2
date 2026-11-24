# Issue #1266 — WebKit: pandoc-warm harness spec hangs in CI since #1210

- **GitHub**: https://github.com/quarto-dev/q2/issues/1266
- **Reporter**: @cscheid (Carlos Scheidegger), 2026-11-19
- **Triage date**: 2026-11-24
- **Branch**: topic branch `issue-1266` in the main checkout (no worktree, by request), based on `main` @ `ecf1bec29`
- **Braid strand**: bd-gogn12rc (existing; filed independently 2026-11-24 for the same hang, see § Outcome)
- **Scope**: the WebKit hang of `hub-client/e2e/pandoc-warm.harness.spec.ts` in the
  "Run pandoc harness tests in WebKit" CI step, and re-enabling the spec. The WebKit
  *timing*-spec flakes that predate #1210 (bd-mc451ftq) are related, but they are a
  separate strand.

## Summary

Since #1210 (optional sign-in, merged 2026-11-18 as `234332037`), CI's Linux WebKit has
hung in the pandoc-warm correctness matrix on nearly every run. This is a real hang, not a
slowdown, and it does **not** reproduce on macOS WebKit (Playwright webkit-2388). Both the
correctness-matrix group and the whole spec pass locally. Triage narrowed what #1210 changed
on the page the harness boots, and found one place in `PandocRunner.run` that can wait
forever. That place explains why the failure is a 15-minute hang and not a diagnosable
error. The root cause inside WebKit is not identified. Identifying it needs an instrumented
CI run, because CI's Linux WebKit is the only known environment that shows the hang.

## Reproduction

Locally (macOS 26 / Darwin 25.6, Apple silicon), it does not reproduce:

```bash
# from the repo root; builds the VITE_E2E bundle the way hub-client-e2e.yml does
(cd hub-client && npm run build:wasm)
# The spec skips WebKit unconditionally; for these runs the skip was made overridable
# (`&& !process.env.ISSUE_1266`) in the working copy only (not committed).
./claude-notes/issue-reports/1266/repro.sh -g "correctness matrix" --reporter=line
#   -> 5 passed (52.4s)                          local-run-1.log
ISSUE_1266=1 npx playwright test --config playwright.harness.config.ts \
  e2e/pandoc-warm.harness.spec.ts --project=webkit --retries=0 --workers=1
#   -> 9 passed, 4 skipped (timing, opt-in) (1.7m)   local-run-2-full-spec.log
```

CI evidence (the `hub-client-e2e.yml` chromium job, WebKit step, dot reporter):

| Run | Commit | Correctness-matrix region |
|---|---|---|
| 44298883680 (green, pre-#1210) | `c7eb54559` | the block of about 33 tests that includes the matrix runs 19:09 → 22:09, **≈3 min** |
| 44339178173 (red, #1210 merge) | `234332037` | 19:19 → 37:18: one test hits the **900 s** timeout (`T`); the rest of its serial group is skipped |
| 44414247441 (red) | `f9936b08d` | same pattern; same WebKit 27.2 probe line in both runs |

A test that normally finishes well inside 3 min and then runs past 15 min has hung.
Slowness would not produce that gap. In the same red runs, the smoke tests at the top of
the spec pass. Those include a single fresh `runRequest` of `callouts-typst`, so the fresh
path works in isolation.

## Localization

**What #1210 changed on the harness's boot page (`/`).** The harness runs `vite preview`
with no hub (`VITE_DISABLE_HUB_PROXY=1`, `playwright.harness.config.ts:84-90`), and CI
builds with `VITE_DEFAULT_SYNC_SERVER=/ws` (`.github/workflows/hub-client-e2e.yml:174`).

- **Before:** `createCollection` waited 10 s for the server, failed, and released the
  server, which disconnected the WebSocket adapter. The page then sat on the static
  `ProjectSetError` card and did nothing else.
- **After** (`hub-client/src/services/projectSetService.ts`, `createCollection` and
  `applySyncGate`): the collection is created locally (IndexedDB) without waiting. The
  WebSocket adapter stays attached and retries `/ws` every 5 s
  (`WebSocketClientAdapter.retryInterval`) for the life of the page. The app renders
  ProjectsHome (the "No projects yet" screen in the failure screenshot) along with the
  new auth/landing effects in `App.tsx`.

So the pandoc workers now share the page with continuing app activity: periodic WebSocket
retries, IndexedDB writes from the local repo, and ProjectsHome's mount work. Before
#1210 they shared it with an idle error card. This difference in page state is the main
lead.

**Why a stall becomes a 15-minute hang.** In `hub-client/src/pandoc/pandocRunner.ts`,
`run()` (around lines 202–310):

- The wall timeout (`DEFAULT_WALL_TIMEOUT_MS`, 120 s) is armed only when the worker posts
  `ready` (`timer = setTimeout(...)` inside the `msg.type === 'ready'` branch).
- Before `ready`, the run waits on `worker.onerror` / `onmessageerror` and nothing else.
  If a fresh worker never answers `init` (module receive or instantiate stalls silently),
  `run()` never settles.
- `await loading` (`PandocLoader.load`) has no time bound either.

The "fresh" mode the hang is attributed to is `getPandoc().runner`. That is a
`PandocRunner`, which creates **a new worker and posts the compiled ~59 MB `Module` on
every render**. The correctness-matrix test does 20 such renders per format (6 typst
recordings + 14 argv-matrix cases). Warm mode reuses pooled workers, so it starts far
fewer workers. Each fresh worker start is a chance to hit a stall, and once one stalls
nothing ends it.

## Open questions — resolved during triage

1. **Is it a hang or a slowdown?** It is a hang. The CI timings in the table above show
   the matrix region going from about 3 min to past the 900 s limit, with the same WebKit
   build.
2. **Does it reproduce locally?** No. macOS WebKit (Playwright webkit-2388) passes both the
   correctness-matrix group and the whole spec (see the two local logs). The trigger is
   specific to CI's Linux WebKit 27.2, or to that runner's load.
3. **Does it happen in the first fresh sequence, as the issue says?** Not established.
   Line 160 sits inside the `for (const format of ['typst', 'typst-pdf'])` loop, so the
   failing `page.evaluate` at :160 can be the **second** fresh sequence (`typst-pdf`), which
   runs after the first warm leg. The issue's "it never reaches the warm-worker code" is
   therefore unproven. The instrumentation below settles it.
4. **Is bd-gogn12rc a different bug?** No, it is the same hang. Its run (PR #1286, commit
   `9290d10ea`) contains #1210 but predates the skip from #1267
   (`git merge-base --is-ancestor`: has `234332037`, lacks `cec7b96d4`).
5. **Pre-flight.** The first `cargo xtask verify --skip-hub-build` failed in hub-client's
   wasm vitest suite because of a stale local `wasm-quarto-hub-client` build. After
   `npm run build:wasm`, all 509 tests (48 files) pass. HEAD is green.

**Escalated (cannot be answered locally):** which part of #1210's boot activity stalls the
worker, and at which stage (load, worker start / `init` → `ready`, or inside the run).
Answering it takes a CI run; see the recommended next step.

## Outcome / recommended next step

Real work. Tracked on the existing strand **bd-gogn12rc**, updated to reference this issue
and doc; no new strand was filed.

Recommended fix path:

1. **Instrument and bound the fresh path** (a product fix that is worth having on its own):
   arm a startup deadline in `PandocRunner.run` covering `init → ready` (and the load
   wait). On expiry, fail with a distinct kind (e.g. `worker-start-timeout`) instead of
   hanging. In the harness, record per-run `{doc, format, stage reached, ms}`. A red CI
   run then names the stalled stage and document instead of reporting a bare 900 s
   timeout.
2. **Experiment branch** with the WebKit skip lifted (in CI only), run 3–4 times:
   - (a) the instrumentation alone;
   - (b) plus a harness-only boot that keeps the app quiet (no sync adapter, no root
     establishment). Options: gate `useAutoEstablishRoot` / `acquireServer` on a test flag,
     or let `test-hooks` close the sync gate (`setSyncAllowed(false)`) before the page
     renders.

   If (b) is green and (a) is red, the boot activity is confirmed. Bisect further among the
   WS retry loop, IndexedDB repo writes, and ProjectsHome.
3. Fix the confirmed cause. Keep the startup deadline either way, then remove the
   `test.skip` at the top of `pandoc-warm.harness.spec.ts`.

## Verification commands used

```bash
gh issue view 1266 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build            # failed: stale hub-client wasm
(cd hub-client && npm run build:wasm && npx vitest run --config vitest.wasm.config.ts)   # 509 passed
git diff 234332037^1 234332037 -- hub-client/src/App.tsx hub-client/src/services/projectSetService.ts \
  hub-client/src/services/syncGate.ts
gh run view 44414247441 --repo quarto-dev/q2 --job 135593504440 --log
gh run view 44298883680 --repo quarto-dev/q2 --log   # green baseline timings
gh run view 44339178173 --repo quarto-dev/q2 --log   # first red run (#1210 merge)
./claude-notes/issue-reports/1266/repro.sh -g "correctness matrix" --reporter=line
braid search pandoc-warm; braid show bd-gogn12rc; braid show bd-mc451ftq
git merge-base --is-ancestor cec7b96d4 9290d10ea; git merge-base --is-ancestor 234332037 9290d10ea
```

## Cross-references

- bd-gogn12rc: same hang, observed on PR #1286's pre-skip base
- bd-mc451ftq: earlier WebKit warm *timing*-spec flakes (2026-10-30, 2026-11-09), before #1210
- #1210 (the trigger), #1267 (the WebKit skip)
- `.claude/rules/hub-client-tests.md` § "Test hooks wait for the state too"
- `dev-docs/pandoc-wasm-host.md` (verified browser versions)
