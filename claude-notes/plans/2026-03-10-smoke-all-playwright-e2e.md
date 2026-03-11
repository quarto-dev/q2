# Smoke-All Tests in Playwright E2E

**Date**: 2026-03-10
**Status**: Phases 1-5 complete, Phase 6 (CI) pending
**Depends on**: Phase 6 items from `2026-01-27-hub-client-testing-infrastructure.md`

---

## Goal

Run the existing smoke-all test fixtures (`crates/quarto/tests/smoke-all/`) through
the real Quarto Hub pipeline in a browser, using Playwright. This catches bugs that
the WASM Vitest smoke-all tests miss because they bypass Automerge and manually
populate the VFS.

**Motivating bug**: A theme array with custom SCSS in a project subdirectory works
in the WASM smoke-all tests (which load all files directly into VFS) but fails in
actual Quarto Hub (no theme applied at all). We cannot currently reproduce this.

## How to verify progress

Each phase is independently testable. After completing a phase, run:

```bash
cd hub-client
npx playwright test          # Run all e2e tests
npx playwright test smoke    # Run just the smoke tests
npx playwright test --ui     # Interactive UI mode for debugging
```

Phase-specific verification:
- **Phase 1**: `npx playwright test smoke` — smoke tests pass, hub server starts/stops
- **Phase 2**: `npx playwright test` — project creation test loads editor + preview
- **Phase 3**: `npx playwright test` — HTML/CSS extraction tests pass
- **Phase 4**: `npx playwright test theme` — theme-subdir test fails (expected!)
- **Phase 5**: `npx playwright test smoke-all` — all smoke-all tests run

**Important**: The hub binary must be built first: `cargo build --bin hub`
The WASM must be built first: `cd hub-client && npm run build:wasm`

## Architecture

```
smoke-all .qmd fixtures on disk (single source of truth)
        │
        ▼
  Playwright test runner (Node.js side)
    ├── discovers .qmd files, parses _quarto.tests frontmatter
    ├── reads all project files from disk
    ├── creates Automerge project via quarto-sync-client → hub server
    │
    ▼
  Browser (Chromium)
    ├── hub-client connects to hub server (WebSocket)
    ├── Automerge sync populates VFS via callbacks
    ├── navigates to project file via URL hash
    ├── waits for preview iframe to render
    │
    ▼
  Assertions
    ├── extract HTML from preview iframe
    ├── extract CSS from VFS artifact via page.evaluate
    ├── extract diagnostics via page.evaluate
    └── run regex/selector assertions from _quarto.tests metadata
```

## Key reference files

Read these before starting implementation:

| File | Why |
|------|-----|
| `ts-packages/sync-test-harness/src/server-manager.ts` | Pattern for starting/stopping hub server — adapt for e2e |
| `ts-packages/sync-test-harness/src/sync-test-helpers.ts` | Pattern for creating projects via quarto-sync-client |
| `ts-packages/quarto-sync-client/src/client.ts` | The sync client API (`createNewProject`, `connect`, etc.) |
| `ts-packages/quarto-automerge-schema/src/index.ts` | Automerge document schema (IndexDocument, TextDocumentContent) |
| `hub-client/src/services/projectStorage.ts` | IndexedDB API for project entries (`addProject()`) |
| `hub-client/src/types/project.ts` | `ProjectEntry` type definition |
| `hub-client/src/utils/routing.ts` | URL scheme (`#/project/<id>/file/<path>`) |
| `hub-client/src/components/DoubleBufferedIframe.tsx` | Render marker comment (line ~172) |
| `hub-client/src/utils/iframePostProcessor.ts` | CSS post-processing (link→data URI conversion) |
| `hub-client/src/services/smokeAll.wasm.test.ts` | WASM smoke-all test — port assertions from here |
| `hub-client/src/services/wasmRenderer.ts` | WASM renderer wrapper (VFS access, render API) |

## Important: npm workspace structure

This project uses npm workspaces. The root `package.json` manages all dependencies.

- `@quarto/quarto-sync-client` → `ts-packages/quarto-sync-client/`
- `@quarto/quarto-automerge-schema` → `ts-packages/quarto-automerge-schema/`

Always run `npm install` from the **repo root**, never from hub-client.

## Important: existing e2e files to preserve

These files belong to the older `2026-01-27-hub-client-testing-infrastructure.md`
plan and must NOT be deleted (except `theme-subdir.spec.ts`):

- `e2e/helpers/fixtureSetup.ts` — fixture copy helper (used by other future e2e tests)
- `e2e/fixtures/testProjects.ts` — test project content definitions
- `e2e/scripts/regenerate-fixtures.ts` — fixture regeneration script

## Design Decisions

### Use existing hub server and sync infrastructure (not a new dependency)

The project already has everything needed:

- **Rust hub server** (`crates/quarto-hub/`) — the production Automerge sync server,
  started via `cargo run --bin hub`. This is what real users hit.
- **`ts-packages/sync-test-harness/src/server-manager.ts`** — `startHubServer()`
  spawns the hub as a child process with temp data dir, readiness polling, and cleanup.
- **`ts-packages/sync-test-harness/src/sync-test-helpers.ts`** — `createTestProject()`
  uses `@quarto/quarto-sync-client` to create Automerge projects on the server.

The E2E `globalSetup` should use `startHubServer()` instead of inventing a new
sync server implementation. No new npm dependency is needed.

### Pre-seed IndexedDB for project loading

After creating the Automerge project on the server (Node.js side), the test must
also register the project in hub-client's IndexedDB (`projectStorage`). Without this,
navigating to a share URL would trigger the connect dialog UI flow, which is fragile
and not what we're testing.

Strategy: use `page.evaluate()` to call `projectStorage.addProject()` with the
`indexDocId` and `syncServer` URL, then navigate to the project by its local ID.
This exercises the full Automerge sync → VFS → render → preview pipeline while
skipping only the manual "connect to project" dialog.

### Dynamic fixture creation (not pre-generated)

Smoke-all fixtures are living source files that change with the codebase.
Pre-generating Automerge snapshots would require a regeneration step every time
a fixture changes. Instead, we create Automerge projects dynamically at test time
by reading files from disk and pushing them through the sync server.

### URL-based navigation (not sidebar clicks)

Navigate to files via `#/project/<localId>/file/<path>` rather than clicking
through the sidebar. This is more robust, faster, and still exercises the full
Automerge → VFS → render → preview pipeline. Sidebar interaction testing is left
for general e2e tests (old plan Phase 6).

### Assertion extraction strategy

- **HTML**: `frame.content()` on the preview iframe for raw HTML string (ensureFileRegexMatches)
- **HTML elements**: `frame.locator(selector)` for CSS selector assertions (ensureHtmlElements)
- **CSS**: `page.evaluate` → read CSS by parsing `<link rel="stylesheet">` hrefs from
  the preview HTML and reading each referenced file from VFS (matching the WASM test
  approach in `smokeAll.wasm.test.ts`).
- **Diagnostics**: `page.evaluate` to read the last render result's diagnostics array (printsMessage, noErrors)
- **Filesystem assertions**: skip (same as WASM tests — no real filesystem in browser)

### Delete theme-subdir.spec.ts

The existing `hub-client/e2e/theme-subdir.spec.ts` was an experiment that bypasses
Automerge entirely (calls wasmRenderer directly via page.evaluate). It did NOT
reproduce the motivating bug. It should be deleted as part of Phase 1 cleanup —
the Phase 4 test through the full Automerge pipeline replaces it.

## Work Items

### Phase 1: Hub server infrastructure for E2E

Reuse existing infrastructure from `ts-packages/sync-test-harness/`.

- [x] Rewrite `e2e/helpers/syncServer.ts` to use `startHubServer()` from
      `sync-test-harness/server-manager.ts` (import or adapt the pattern).
      The hub server is started via `cargo run --bin hub -- --data-dir <tmpdir> --port <port>`.
      Wait for "Hub server listening" in stdout before considering it ready.
- [x] Update `e2e/helpers/globalSetup.ts` to start the hub server. Store the
      server URL in a **file** (e.g., `/tmp/hub-e2e-server.json`) because Playwright
      globalSetup runs in a separate process from test workers — `globalThis` and
      env vars set in globalSetup are NOT visible to tests. Use Playwright's
      recommended pattern: write to a well-known file, read from tests.
- [x] Update `e2e/helpers/globalTeardown.ts` to stop the hub server and clean up.
- [x] Delete `hub-client/e2e/theme-subdir.spec.ts` (obsolete experiment)
- [x] Verify globalSetup starts the hub server and globalTeardown stops it.
      Run `npx playwright test smoke` and check the server starts/stops in console output.
- [x] Update `e2e/smoke.spec.ts` to read the server URL from the file and verify
      it's a valid WebSocket URL.

**Verification**: `cd hub-client && npx playwright test smoke` passes.

### Phase 2: Project creation and loading through Automerge

- [x] Write a helper `createProjectOnServer(serverUrl, files[])` in
      `e2e/helpers/projectFactory.ts`. Use `@quarto/quarto-sync-client`'s
      `createSyncClient()` + `createNewProject()` (same pattern as
      `sync-test-helpers.ts::createTestProject()`). Returns the `indexDocId`.
      The helper must:
      1. Create a sync client with no-op callbacks
      2. Call `client.createNewProject({ syncServer: serverUrl, files })`
      3. Wait 2 seconds for server persistence
      4. Call `client.disconnect()`
      5. Return `result.indexDocId`
- [x] Write a helper `seedProjectInBrowser(page, { indexDocId, syncServer, name })`
      in `e2e/helpers/projectFactory.ts`. Uses `page.evaluate()` to call
      `projectStorage.addProject(indexDocId, syncServer, name)` via Vite's
      dynamic import (`await import('/src/services/projectStorage.ts')`).
      Returns `entry.id` (the local UUID used in URLs).
- [x] Write `e2e/project-loading.spec.ts` that:
      1. Creates a simple project (single .qmd + _quarto.yml) on the server
      2. Navigates to app root (`/`) first to initialize the page
      3. Seeds the project in the browser's IndexedDB
      4. Navigates to `#/project/<localId>/file/index.qmd`
      5. Waits for preview iframe to appear
      6. Verifies rendered content contains expected text

**Verification**: `cd hub-client && npx playwright test project-loading` passes.

### Phase 3: Preview content extraction

- [x] Write helper `waitForPreviewRender(page, opts?)` in `e2e/helpers/previewExtraction.ts`.
      Polls for `iframe.preview-active` with non-empty body innerHTML.
      Default timeout 30s. Returns when render is detected.
- [x] Write helper `getPreviewHtml(page)` that returns raw HTML from the
      preview iframe. Must handle the DoubleBufferedIframe pattern (two iframes,
      one active with class `preview-active`).
- [x] Write helper `getPreviewCss(page)` that:
      1. Gets preview HTML from active iframe
      2. Parses `<link rel="stylesheet" href="...">` tags
      3. Handles data: URIs (CSS inlined by iframePostProcessor) and VFS reads
      4. Returns concatenated CSS string
- [x] Write helper `getRenderDiagnostics(page, documentPath)` that re-renders
      via `page.evaluate` → `renderToHtml()` to capture diagnostics.
- [x] Write `e2e/preview-extraction.spec.ts` that creates a basic project,
      navigates to it, and asserts on HTML content using regex.

**Verification**: `cd hub-client && npx playwright test preview-extraction` passes.

### Phase 4: Reproduce the theme-subdir bug

This is the proof-of-concept milestone.

- [x] Write `e2e/theme-subdir-e2e.spec.ts` that:
      1. Creates a project with files from the smoke-all fixture at
         `crates/quarto/tests/smoke-all/themes/theme-array/` (read from disk)
      2. Seeds the project in the browser
      3. Navigates to `subdir/theme-array-subdir.qmd`
      4. Waits for render
      5. Extracts CSS and asserts on `#170229`, `smoke-test-subdir-rule`, `#def456`
- [x] **Result**: Test PASSES — the bug is NOT reproduced. The SASS cache fix
      (47a569ef) may have resolved the underlying issue, or the Automerge
      pipeline correctly handles subdirectory themes now.
- [ ] ~~Investigate and document the root cause based on the failure~~ (N/A — passes)

**Verification**: `cd hub-client && npx playwright test theme-subdir-e2e` — PASSES.

### Phase 5: Smoke-all test runner

- [x] Write `e2e/helpers/smokeAllDiscovery.ts` — discover .qmd files, parse
      frontmatter, find project roots. Port discovery logic from
      `smokeAll.wasm.test.ts` (functions: `discoverTestFiles`, `readFrontmatter`,
      `findProjectRoot`, `readAllFiles`).
- [x] Write `e2e/helpers/smokeAllAssertions.ts` — port assertion logic from
      `smokeAll.wasm.test.ts`. Adapt to use Playwright's `page`/`frame` instead
      of direct WASM calls. Key functions to port:
      - `makeEnsureFileRegexMatches` → regex on `getPreviewHtml()`
      - `makeEnsureCssRegexMatches` → regex on `getPreviewCss()`
      - `makeEnsureHtmlElements` → `frame.locator(selector)` (first native browser impl)
      - `assertNoErrors` / `assertNoErrorsOrWarnings` → `getRenderDiagnostics()`
      - `assertShouldError` → check render failure
      - `makePrintsMessage` → diagnostics array + regex
- [x] Write `e2e/smoke-all.spec.ts` that:
      1. Discovers all .qmd files under `crates/quarto/tests/smoke-all/`
      2. For each, parses frontmatter to extract `_quarto.tests` metadata
      3. Finds project root (walk up for `_quarto.yml`)
      4. Reads all project files from the project root
      5. Generates a Playwright test dynamically (one test per fixture)
      6. Each test: create project → seed → navigate → wait → assert
- [x] Handle run config (skip, ci, os filtering) — port `shouldSkip()` from
      `smokeAll.wasm.test.ts`
- [x] Handle format filtering (only test `html` format, same as WASM tests)
- [x] Run all smoke-all tests; document which pass and which fail
      **Result**: All 23 smoke-all tests pass (34 total including other e2e tests).
      `SKIP_PRINTS_MESSAGE` set used for `quarto-test/expected-error.qmd`
      (same as WASM test). Source-tracking spans stripped before regex matching.

**Verification**: `cd hub-client && npx playwright test smoke-all` runs all fixtures.

### Phase 6: CI integration

- [ ] Update `.github/workflows/hub-client-e2e.yml` to include smoke-all tests
- [ ] Ensure hub binary is built before smoke-all tests run (`cargo build --bin hub`)
- [ ] Ensure WASM is built before smoke-all tests run
- [ ] Set appropriate timeouts (SCSS compilation tests are slow; hub server
      compilation on first run can take >60s)
- [ ] Add smoke-all test results to the Playwright HTML report

## Shared helpers to create

These go in `e2e/helpers/`:

| Helper | Purpose |
|--------|---------|
| `syncServer.ts` | Start/stop hub server (rewrite stub to use `startHubServer()` pattern) |
| `projectFactory.ts` | `createProjectOnServer()` + `seedProjectInBrowser()` |
| `smokeAllDiscovery.ts` | Discover .qmd files, parse frontmatter, find project roots |
| `previewExtraction.ts` | `waitForPreviewRender()`, `getPreviewHtml()`, `getPreviewCss()`, `getRenderDiagnostics()` |
| `smokeAllAssertions.ts` | Port assertion logic from `smokeAll.wasm.test.ts` |

## Assertion portability reference

| Assertion | Source | Playwright approach |
|-----------|--------|-------------------|
| `ensureFileRegexMatches` | HTML string | `getPreviewHtml()` + regex |
| `ensureCssRegexMatches` | CSS artifact | `getPreviewCss()` + regex |
| `ensureHtmlElements` | CSS selectors | `frame.locator(selector)` — native Playwright |
| `noErrors` | Render result | `getRenderDiagnostics()` → check no errors |
| `noErrorsOrWarnings` | Render result | `getRenderDiagnostics()` → check empty |
| `shouldError` | Render result | Check render failure state |
| `printsMessage` | Diagnostics | `getRenderDiagnostics()` + regex |
| `fileExists` | Filesystem | Skip (no filesystem in browser) |
| `folderExists` | Filesystem | Skip |
| `pathDoesNotExist` | Filesystem | Skip |

## Old plan items completed by this work

When this plan is done, check off these items in
`2026-01-27-hub-client-testing-infrastructure.md`:

**Phase 1:**
- `[ ] Add @automerge/automerge-repo-sync-server dependency for local E2E sync server`
  → Replaced by using the existing hub server; no new dependency needed.

**Phase 6:**
- `[ ] Set up Playwright configuration with fixture lifecycle hooks`
- `[ ] Add tests for project loading (using fixture with known docId)`
- `[ ] Add tests for SCSS compilation and caching behavior`
- `[ ] Add tests for preview rendering`

Items NOT covered (remain for future work):
- `[ ] Add tests for project creation flow (fresh documents)`
- `[ ] Add tests for file editing flow`

## Resolved questions

### Render completion signal

The `DoubleBufferedIframe` component (used by Preview) injects a unique
`<!-- render-<timestamp> -->` comment into the HTML on each render
(`DoubleBufferedIframe.tsx:172`). The E2E wait strategy:

1. Wait for `iframe.preview-active` to exist in the DOM
2. Wait for a `<!-- render-XXX -->` comment to appear in its content
3. Allow ~50ms for CSS post-processing (data URI conversion by `iframePostProcessor.ts`)

The Preview component also has a state machine (`START` → `GOOD` | `ERROR_AT_START`)
but the render comment is more directly observable from Playwright.

Write a helper `waitForPreviewRender(page, opts?)` in `previewExtraction.ts` that
implements this polling loop with a configurable timeout.

### projectStorage API shape

```typescript
// projectStorage.ts — the function we'll use
addProject(indexDocId: string, syncServer: string, description?: string): Promise<ProjectEntry>
```

Auto-generates `id` (UUID), `createdAt`, `lastAccessed`. Returns the full
`ProjectEntry` including the local `id` needed for URL navigation. No
initialization needed — the database opens lazily.

The `seedProjectInBrowser()` helper:
1. `page.evaluate()` → `const ps = await import('/src/services/projectStorage.ts')`
2. `const entry = await ps.addProject(indexDocId, syncServer, 'E2E Test')`
3. Return `entry.id` for URL construction (`#/project/<id>/file/<path>`)

### quarto-sync-client in Node.js context

`BrowserWebSocketClientAdapter` is misnamed — the automerge docs say "both
implementations work in both the browser and on node via `isomorphic-ws`."
The adapter uses `isomorphic-ws` which resolves to the `ws` npm package in
Node.js (already a devDependency). No polyfill needed. The existing
`sync-test-harness` already uses this from Node.js vitest.

### Per-test project creation

Each smoke-all test creates its own Automerge project. This is slower than
sharing projects but avoids ordering dependencies and VFS contamination.
Start with this approach and optimize only if performance is a problem.

## Open questions

1. **Project creation timing**: After creating an Automerge project on the server
   via `quarto-sync-client`, the creator must disconnect and the data must be
   persisted before the browser connects. The existing `sync-test-helpers.ts` uses
   a 2-second sleep after creation. Need to verify this is sufficient or find a
   more deterministic signal.

2. **VFS access from page.evaluate**: The wasmRenderer module is in ES module scope.
   We've confirmed that `await import('/src/services/wasmRenderer.ts')` works via
   Vite's dev server. Need to verify this works reliably for reading VFS and
   diagnostics.

3. **Parallel test isolation**: Each smoke-all test creates its own Automerge
   project with a unique indexDocId. But the hub-client app has module-level VFS
   state. If tests run in parallel (different browser contexts), each gets its own
   WASM instance and VFS. Need to verify Playwright's parallel workers give proper
   isolation.

4. **Hub server compilation time**: The first `cargo run --bin hub` in CI may need
   to compile. The existing `server-manager.ts` uses a 120s timeout for this. In
   the E2E `globalSetup`, we should either pre-build the binary or use a similarly
   generous timeout.
