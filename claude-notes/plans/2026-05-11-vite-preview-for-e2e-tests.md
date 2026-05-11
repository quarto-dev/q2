# Switch hub-client e2e to `vite preview` instead of `vite dev`

**Date:** 2026-05-11
**Branch:** `chore/e2e-ci` (PR #172) — stay on this branch, validate by pushing to the open PR
**Worktree:** `.worktrees/e2e-ci`
**Status:** Planned — not started

## Overview

Goal: cut wall-clock and reduce flakiness in the `Hub-Client E2E Tests`
workflow by serving the hub-client to Playwright from a prebuilt bundle
instead of vite's dev server.

### Motivation

Each Playwright test creates a fresh browser context, which downloads
roughly:

| Asset | Size (uncompressed) |
|---|---|
| `wasm_quarto_hub_client_bg.wasm` | **32 MB** |
| `automerge_wasm_bg.wasm` | 1.8 MB |
| `web-tree-sitter.wasm` | 192 KB |
| dart-sass dynamic-import bundle | ~5 MB |
| Monaco editor chunks | ~3 MB |
| Hundreds of small TS/JSX modules | ~5 MB total |

Per fresh context, **~50 MB of bytes through `vite dev`**, served by a
single-threaded dev server that also has to run its plugin pipeline on
every TS/JSX module on demand. With 2 Playwright workers contending for
one dev server on a 2-core runner, the cold-context page load can hold
up another worker's test for tens of seconds.

Working theory: the "preview iframe didn't render in 45s" timeouts we
keep hitting (and the 5-10 "flaky" retries per run) are mostly **page
loads** blocked on vite dev's serialized module pipeline + uncompressed
binary serving, not actual render time. A static prebuilt bundle served
via `vite preview` should remove this whole class of contention:

- Gzip/brotli compression for binary assets (32 MB → ~8-12 MB on the wire)
- No transform pipeline → no per-request serialization point
- ~10 bundled JS chunks vs. ~500 separate dev-mode module requests
- HTTP cache reuse across same-worker tests is more predictable

### Target outcome

| Metric | Current | Target |
|---|---|---|
| Workflow total | ~16 min | sub-12 min |
| `Run E2E tests` step | ~7 min | sub-5 min |
| Flaky tests | 5-10 / run | ≤ 2 / run |
| Hard failures | 0-1 / run | 0 across ≥ 3 runs |

## Approach

Stay on `chore/e2e-ci`. Each iteration: edit, build locally, run smoke-all
locally with 2 workers, then push to PR #172 to validate on CI. The
existing 75s preview-render timeout (commit `81cc5264`) is a safety net
during the switch; once preview is stable, drop it back to 45s in the
same series.

## Work items

### Phase 0 — Baseline measurement (so we know we improved)

- [ ] Record current local timing: `cd hub-client && CI=1 npx playwright test --grep smoke-all --workers=2 --reporter=line --timeout=90000` (with current `vite dev`). Capture: total wall, flaky count, slowest 5 tests.
- [ ] Record current CI timing from the most recent green run (`25647967388`): total job, `Run E2E tests`, `Build TypeScript packages` durations.

### Phase 1 — Vite preview config

- [ ] `hub-client/vite.config.ts`: lift the proxy block into a `preview.proxy` mirror (vite preview ignores `server.proxy`). Same target/target-handling/rewrite as the existing `server.proxy`. The shared constant `hubTarget` is fine to reuse.
- [ ] Confirm `vite build` emits the WASM file as a static asset under `dist/assets/` (the local `dist/` already shows `wasm_quarto_hub_client_bg-<hash>.wasm` and `automerge_wasm_bg-<hash>.wasm`, so this works today).
- [ ] Verify HTTP `Content-Encoding: gzip` is sent for the WASM by `vite preview` by hitting it manually: `cd hub-client && npm run build && npm run preview -- --port 5173 &; curl -sI http://localhost:5173/assets/wasm_quarto_hub_client*.wasm -H 'Accept-Encoding: gzip'`. If preview doesn't gzip by default, decide whether to: (a) accept the uncompressed 32 MB and rely on the no-transform-pipeline win alone, (b) front it with `compression` middleware via a small Express wrapper, or (c) live with current size and focus on the other wins.

### Phase 2 — Playwright wiring

- [ ] `hub-client/playwright.config.ts`: change `webServer.command` from `npm run dev` to `npm run preview -- --port 5173`. Keep `url: 'http://localhost:5173'`. Keep the `reuseExistingServer` setting. Bump `timeout` if 120s isn't enough to cover the preview-server startup (it should be near-instant).
- [ ] Decide where `VITE_HUB_SERVER` is set:
  - Option A: bake it into the build (set on the workflow's "Build TypeScript packages" step env). Cleanest. The hub URL would be hard-coded into the built JS, which is fine for CI but not for shareable artifacts.
  - Option B: read it at runtime via vite preview's `preview.proxy` config. Vite reads `process.env.*` at config-eval time when starting the preview server, so passing it via the `webServer.env` in playwright.config.ts should work.
  - Pick Option B if it works — keeps the build artifact reusable; only the preview-server invocation needs the env.
- [ ] Local run: same command as Phase 0 baseline. Confirm tests pass, capture timings.

### Phase 3 — CI integration

- [ ] If we used Option B above: no workflow change beyond what's already there.
- [ ] If we used Option A: add `VITE_HUB_SERVER: http://localhost:3030` to the "Build TypeScript packages" step env in `.github/workflows/hub-client-e2e.yml`.
- [ ] Push to `chore/e2e-ci`. Watch the new PR-triggered run. Compare against the Phase 0 CI baseline.

### Phase 4 — Iterate / validate

- [ ] If first run is green with good timings: trigger 2 more runs via `gh workflow run hub-client-e2e.yml --ref chore/e2e-ci` to check stability across consecutive runs.
- [ ] If first run has new failures (production-build code-path differences, missing assets, broken proxy): diagnose, fix, push again.
- [ ] If the preview switch alone clears flakes: drop the 75s preview timeout back to 45s in a follow-up commit to confirm it's the preview mode (not the timeout headroom) that's doing the work.

### Phase 5 — Cleanup

- [ ] If preview migration sticks, consider squashing the timeout-bump commit (`81cc5264`) into the preview commit via `git revise` — or keep them separate so the bisect history tells the story.
- [ ] Update the squashed CI commit's message (or add a fresh commit) describing the dev→preview switch.
- [ ] Update the PR description / comment with before/after timing numbers.
- [ ] If a measurable speed-up landed, add a sentence to `hub-client/playwright.config.ts` explaining why it's `preview` (so a future agent doesn't "helpfully" revert it to `dev` for HMR).

## Risks / unknowns

- **`preview.proxy` may not honor every option of `server.proxy`**. The shared proxy logic should be straightforward (forward `/auth/*` and the ws upgrade to `http://localhost:3030`), but worth checking against vite docs and an actual local run before pushing.
- **Production-only code paths in hub-client**. If anything reads `import.meta.env.PROD` and behaves differently in built mode, e2e would suddenly hit it. Could surface real bugs (good!) or break tests for non-bug reasons (need fixing). The hub-client doesn't seem to have many of these from a quick scan, but full local-run validation in Phase 1 catches it.
- **Source maps and debugging**. Built mode has external source maps; failure investigation needs `playwright show-trace` or careful artifact-spelunking, same as today. Likely no regression.
- **The `optimizeDeps.exclude: ['wasm-quarto-hub-client']` line is dev-only**; vite build handles the alias via `resolve.alias` regardless. The local `dist/` already proves this works.
- **Cache busting**. Vite build produces hashed filenames. If anything in test setup or `e2e/helpers/` hard-codes a JS asset path (it shouldn't — everything goes through the HTML entry), it'd break.
- **HMR is gone in preview** — fine for CI, not a regression because we never used HMR in tests.
- **First-test-in-worker is still slower than subsequent** (browser cache cold), but the absolute number should be much smaller. Acceptable.

## Success criteria

The PR's next push triggers a CI run that:

1. Completes the `Run E2E tests` step in **under 5 minutes**.
2. Reports **0 hard failures**.
3. Reports **≤ 2 flaky tests**.
4. Total workflow under **12 minutes**.

Then run the workflow 2 more times via `workflow_dispatch` and confirm the
numbers are stable, not lucky-once.

## Estimated effort

30-90 minutes including local validation. The risk is that the
production build surfaces a hub-client bug that doesn't exist in dev
mode, in which case scope expands.
