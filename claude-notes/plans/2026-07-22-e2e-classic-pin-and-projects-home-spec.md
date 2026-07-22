# E2E: pin classic UI in existing suite + new projects-home spec

**Strand:** bd-cbuc8n0e (discovered-from bd-je3w8q39)
**Branch:** `feature/85-projects-collections-ui` (PR #394)

## Overview

PR #394 makes the collections-based ProjectsHome the default UI variant
(`qh-ui-variant`, defaulting to `'collections'` in `App.tsx`). The whole
Playwright E2E suite predates it and drives the classic ProjectSelector — its
bootstrap waits for the "Your Projects" heading. Result: 53/55 tests failed on
the PR's first E2E run (run 29947880915). The suite had never run on this PR
before because `pull_request` workflows can't run while a PR is CONFLICTING;
resolving the conflict surfaced the latent failure.

Decision (Carlos): the classic variant stays available for a while, so the
existing suite keeps testing it (pinned explicitly), and the collections home
gets its own new spec.

## Design decisions

- **Pin, don't migrate.** `bootstrapProjectSet` seeds
  `localStorage['qh-ui-variant'] = 'classic'` via `addInitScript` (before any
  page JS, which is when `App.tsx` reads it). The two specs with their own
  bootstraps — `import-zip.spec.ts` and `share-link-project-set.spec.ts` — call
  the same exported helper.
- **Shared bootstrap refactor.** The common part of `bootstrapProjectSet`
  (auth stub, Monaco CDN intercept, preferences seed, setup-screen flow) is
  parameterized by UI variant; `bootstrapProjectSet` keeps its exact current
  contract (classic, lands on "Your Projects"), and a new
  `bootstrapProjectsHome` lands on the collections home.
- **New spec drives real UI**, reusing `createProjectOnServer` +
  `seedProjectInBrowser` for data setup (same as every other spec).
- Order assertions use A-to-Z (deterministic); recency order between two
  projects seeded in the same millisecond is not asserted.

## Work items

- [x] `projectFactory.ts`: variant-parameterized bootstrap;
      `bootstrapProjectSet` (classic, unchanged contract) +
      `bootstrapProjectsHome` (collections) + exported `seedUiVariant`.
- [x] `import-zip.spec.ts`, `share-link-project-set.spec.ts`: seed classic
      variant before their own `goto('/')` calls.
- [x] New `e2e/projects-home.spec.ts`:
      - boots into the collections home (New collection button, search box)
      - creates a collection via the dialog
      - moves a project in via ⋯ → Move to collection
      - right-click on a card opens the project context menu
      - per-collection sort button reorders cards (A to Z) and updates its title
- [x] Local verification: `VITE_E2E=1 npm run build`, then targeted
      `npx playwright test projects-home import-zip share-link-project-set`
      (hub auto-started by globalSetup).
- [x] Changelog (two-commit workflow) + `npm run test:wasm`.
- [ ] Push to PR branch (with permission) and confirm the E2E workflow goes
      green.

## Verification record

2026-07-22, local run against `VITE_E2E=1 npm run build` + auto-started hub
(globalSetup, `cargo run --bin hub`):

- `npx playwright test projects-home import-zip share-link-project-set`:
  first pass caught a real bug in the new bootstrap — with an empty project
  set the collections home renders a "No projects yet" empty state without
  the "＋ New collection" button; landing assertion switched to the header
  search box (present in every state). After the fix: **projects-home 2/2
  passed**; import-zip and share-link-project-set passed under the classic
  pin (one share-link test flaky-passed on retry, within this suite's
  configured retry budget).
- Spot-check of the shared `bootstrapProjectSet` path (`project-loading`,
  `search.spec`): 2/2 passed.
- `npx tsc -b` clean.
- No changelog entry: e2e-infra-only change, matching precedent (25345d55);
  the changelog is user-facing.

CI confirmation pending push (last work item).
