---
title: 'Fix ProjectSetSetup Light Theme Button Visibility'
date: 2026-04-07
description: 'Fixes unreadable error text, an invisible backup section and a heavy shadow on the `ProjectSetSetup` migration page in light theme by moving hardcoded colors to theme variables, and adds visual regression tests.'
---

## Overview

The ProjectSetSetup migration page has hardcoded dark-theme colors that were missed during the hub-client color scheme refactor (2026-04-02). Users on light theme see invisible or illegible UI elements.

Since this page only appears when there's unmigrated IndexedDB data (a state we can't easily reproduce manually), we also add a dev route and Playwright visual regression tests so we can verify the fix and catch future regressions.

## Problem

In `hub-client/src/components/ProjectSetSetup.css`, three values are hardcoded for dark backgrounds:

1. **Line 56** — `.setup-error` has `color: #ff8a70` (light salmon). On a light background this is low-contrast and hard to read. Should use a theme variable.

2. **Line 53** — `.setup-error` has `background: rgba(219, 89, 59, 0.15)` (translucent red). On a white modal background this becomes a very faint pink, which is probably fine, but it's still a hardcoded value rather than using the theme system.

3. **Line 63** — `.setup-backup` has `background: rgba(255, 255, 255, 0.05)` (nearly transparent white). This was designed to create a subtle highlight on a dark background. On a light/white modal background, it's completely invisible — the backup section has no visual distinction from its surroundings.

4. **Line 20** — `.setup-modal` has `box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5)`. This is very heavy for light mode. A softer shadow would look better.

The buttons themselves (`.setup-primary-btn`, `.setup-secondary-btn`, `.backup-btn`) all use CSS variables correctly and should render fine. The reported "invisible buttons" may actually be about the backup section blending into the background, making the "Download Backup" button appear to float without context.

## Phase 1: CSS Fix

- [x] Add new variables to both `:root.dark` and `:root.light` in `theme.css`:
  - `--error-text`: dark `#ff8a70`, light `#c0392b`
  - `--error-bg-subtle`: dark `rgba(219, 89, 59, 0.15)`, light `rgba(219, 89, 59, 0.08)`
  - `--bg-subtle`: dark `rgba(255, 255, 255, 0.05)`, light `rgba(0, 0, 0, 0.03)`
  - `--modal-shadow`: dark `0 20px 60px rgba(0,0,0,0.5)`, light `0 20px 60px rgba(0,0,0,0.15)`
- [x] Update `ProjectSetSetup.css` to use the new variables
- [x] Verify `npm run build:all` passes

## Phase 2: Dev Route for Visual Testing

The app uses hash-based routing with custom parsing in `src/utils/routing.ts`. We add a `#/dev/...` route family gated behind `import.meta.env.DEV` so it's stripped from production builds.

- [x] Add `DevRoute` type to `routing.ts`, parse `#/dev/<page>` only when `import.meta.env.DEV` is true, handle in `buildHashRoute` and `routesEqual`
- [x] Create `DevHarness.tsx` with canned props for `setup-migration`, `setup-migration-error`, and `setup-fresh` pages
- [x] Wire into `App.tsx` via `React.lazy()` — DevHarness is code-split into its own 1.5KB chunk, never fetched in production
- [x] Verify `npm run build:all` passes and all 414 unit tests pass
- [ ] Verify the dev routes render correctly in `npm run dev` (deferred to Phase 3 Playwright tests)

## Phase 3: Playwright Visual Regression Tests

- [x] Create `playwright.visual.config.ts` — separate config for visual tests (no hub server needed, no global setup/teardown)
- [x] Add `e2e/setup-screens.visual.spec.ts` with 6 tests: migration/migration-error/fresh-setup x dark/light themes
- [x] Add `test:visual` and `test:visual:update` npm scripts
- [x] Verified: first run creates baselines with `--update-snapshots=missing`, second run passes against them
- [x] Visually inspected screenshots — light theme fix confirmed working (readable error text, visible backup section, appropriate shadow)

## Phase 4: CI Snapshot Workflow

Font rendering differs across platforms (macOS Core Text vs Linux FreeType vs Windows DirectWrite), so visual regression baselines must be pinned to a single platform. We pin to Linux (the CI runner). Developers on macOS/Windows can run the tests locally for advisory feedback, but the authoritative baselines come from CI.

### Playwright snapshot update modes

Playwright's `--update-snapshots` flag has three modes:
- `--update-snapshots=all` (or just `--update-snapshots`) — overwrites **every** snapshot. Dangerous: silently accepts regressions.
- `--update-snapshots=missing` — only creates snapshots that don't exist yet. Existing snapshots are compared normally and can still fail.
- No flag — strict comparison, fails on missing or mismatched.

**We only use `--update-snapshots=missing`.** To update an existing snapshot that has intentionally changed, the developer deletes the stale snapshot file, commits the deletion, and lets the "missing" flow recreate it. This makes every snapshot update intentional and auditable in the git diff.

### Automatic baseline creation on push

When a developer adds a new visual test without its baseline snapshot, CI should automatically create the missing baseline and commit it — no manual intervention needed. The workflow:

1. Run `npx playwright test` normally. If all tests pass, done.
2. If tests fail, re-run with `npx playwright test --update-snapshots=missing`. This creates any missing baselines but **still compares existing snapshots normally**, so real regressions still fail.
3. If the second run passes (the only failures were missing snapshots), commit the new snapshot files back to the branch.
4. If the second run also fails, it's a real regression — don't commit, upload the Playwright report as usual.

This is safe because `--update-snapshots=missing` never overwrites existing baselines. A real visual regression cannot be masked by this flow.

### Manual "recreate all snapshots" escape hatch

For rare situations where all baselines need regeneration (browser version bump, font change, CI image update), the workflow provides a manual trigger with a "Recreate all snapshots" checkbox. This:

1. Deletes all existing snapshot files
2. Runs with `--update-snapshots=missing` (which now recreates everything since they're all "missing")
3. Commits the new baselines

This is deliberately a two-step destructive operation (delete + recreate) rather than using `--update-snapshots=all`, to keep the same single code path.

### GHA changes to `hub-client-e2e.yml`

- [x] Add `workflow_dispatch` input and push trigger:
  ```yaml
  on:
    push:
      branches: [main]
      paths:
        - 'hub-client/**'
    workflow_dispatch:
      inputs:
        recreate-all-snapshots:
          description: 'Delete and recreate ALL visual regression baselines'
          type: boolean
          default: false
  ```

- [x] Add a step to delete all snapshots when "recreate all" is requested:
  ```yaml
  - name: Delete all snapshots (recreate mode)
    if: inputs.recreate-all-snapshots == true
    run: find hub-client/e2e -type d -name '*-snapshots' -exec rm -rf {} + || true
  ```

- [x] Update the workflow to implement auto-retry logic for visual tests:
  ```yaml
  - name: Run E2E tests
    id: e2e
    continue-on-error: true
    run: |
      cd hub-client
      npx playwright test

  - name: Retry with --update-snapshots=missing
    id: e2e-retry
    if: steps.e2e.outcome == 'failure'
    run: |
      cd hub-client
      npx playwright test --update-snapshots=missing

  - name: Commit new baselines
    if: steps.e2e.outcome == 'failure' && steps.e2e-retry.outcome == 'success'
    run: |
      git config user.name "github-actions[bot]"
      git config user.email "github-actions[bot]@users.noreply.github.com"
      git add hub-client/e2e/**/*-snapshots/
      git diff --cached --quiet || git commit -m "Add missing Playwright visual regression baselines"
      git push

  - name: Fail if retry also failed
    if: steps.e2e-retry.outcome == 'failure'
    run: exit 1
  ```

- [x] Keep the existing artifact upload step (on failure) so regressions produce downloadable reports

## Notes

- The dev route infrastructure is generic — future components that are hard to reach (error states, onboarding flows, etc.) can be added to `DevHarness.tsx` with minimal effort.
- Playwright's `toHaveScreenshot()` uses pixel comparison with configurable thresholds, which is well-suited for catching "text same color as background" bugs.
- The e2e tests for setup screens don't need the Rust hub server (no network calls), but they do need the Vite dev server. The existing Playwright config already starts it.
- Visual baselines are pinned to Linux CI. Local runs on macOS/Windows are advisory only — expect font rendering diffs.
