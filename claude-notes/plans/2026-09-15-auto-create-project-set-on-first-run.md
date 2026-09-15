# Auto-create the project set on first run (drop the fresh-setup screen)

**Strand:** bd-4h1hv60p
**Status:** decisions settled 2026-09-15 (see Resolved questions); ready to execute on approval

## Overview

A brand-new hub-client user (no collection pointer in IndexedDB, no
legacy projects) currently lands on `ProjectSetSetup`'s "fresh" screen
after signing in: a card with the tagline, a "Sync Server URL" input
pre-filled with `DEFAULT_SYNC_SERVER`, and two buttons, **Create New
Project Set** and **Link to Existing Project Set**. Every new user has
to click the primary button before they see the app.

This plan removes that stop. When the app finds no project set, it
silently creates the personal root collection against
`DEFAULT_SYNC_SERVER` and goes straight to the collections home, which
already has the right empty state ("No projects yet — Create your first
Quarto project, or connect to one a collaborator shared").

Two code paths already do exactly this, each behind its own gate:

- **Collection/document invites** (bd-fxdcxbpq): `App.tsx` ~L349, an
  effect that fires `createProjectSet(DEFAULT_SYNC_SERVER)` on
  `needs-setup` (and `migrateProjects` on `needs-migration`) while an
  invite landing is showing.
- **Ephemeral `q2 preview` boots** (bd-zf4ryvuq): `App.tsx` ~L451, the
  identical effect gated on `ephemeralHub`. Its plan
  (`2026-08-07-preview-editor-skip-project-setup.md`, decision 2)
  already argued for *silent auto-setup over a bare gate skip* so the
  whole app stays coherent — the same argument applies here.

The change is therefore mostly a **generalization**: make the silent
root establishment the default for everyone, fold the two gated copies
into one hook, and delete the fresh-setup UI that no longer has a
caller. The user-visible path becomes: sign in → skeleton for the
connect → "No projects yet".

## Current behavior (from the code)

| Piece | Where | Notes |
| --- | --- | --- |
| Status machine | `hub-client/src/hooks/useCollectionSets.ts` | `loading → needs-setup \| needs-migration \| connecting → connected \| error`. `needs-setup` = no pointers and no legacy IDB projects. |
| Setup actions | same file | `createProjectSet(syncServer)`, `linkProjectSet`, `migrateProjects`, `mergeIntoProjectSet`; all call `establishRoot` (pointer array + legacy singleton). |
| Gate | `hub-client/src/App.tsx` ~L1043–L1080 | Renders `ProjectSetSetup` on `needs-setup`/`needs-migration` unless `ephemeralHub`; renders it again (fresh mode, with the error) on `error`. |
| Screen | `hub-client/src/components/ProjectSetSetup.tsx` (353 lines) + `.css` (198) | Four modes: fresh, link, migration, merge. |
| Silent copies | `App.tsx` ~L349 (invite) and ~L451 (ephemeral) | Same effect body, two refs, two `eslint-disable` lines. |
| Inbound linking | `App.tsx` ~L597, route `#/link-project-set/<id>?server=` | Route handler calls `linkProjectSet`/`mergeIntoProjectSet`, which *establish the root*. Built by "Link another browser…" in the ProjectsHome avatar menu. |
| Dev harness | `DevHarness.tsx` pages `setup-fresh`, `setup-migration`, `setup-migration-error` | Scanned by `e2e/baseline-a11y.harness.spec.ts`. |
| E2E bootstrap | `e2e/helpers/projectFactory.ts` (`bootstrapProjectSetVariant`), `e2e/import-zip.spec.ts`, `e2e/share-link-project-set.spec.ts` | Fill `#setup-sync-server` with the local hub URL, click **Create New Project Set**. 31 spec files go through the shared helper. |
| Default server | `hub-client/.env` → `wss://sync.automerge.org`; production build sets `VITE_DEFAULT_SYNC_SERVER=wss://public-preview.quarto-hub.com/ws`; preview-embed sets `/ws` | The e2e build does **not** override it — tests reach the local hub only by typing its URL into the setup input. |

## Proposed behavior

1. **`needs-setup` → silent create.** One effect (extracted into a hook)
   fires `createProjectSet(DEFAULT_SYNC_SERVER)` exactly once when
   status enters `needs-setup`. No screen. The home renders its
   skeleton during `connecting` (already implemented at
   `ProjectsHome.tsx` ~L1105) and then the empty state.
2. **Boot-time exception: `#/link-project-set/…`.** If the boot URL is
   an inbound project-set link, the route handler owns setup (as
   today) and the auto-create must not fire — otherwise the linked set
   would be appended as a secondary collection next to a fresh empty
   root. Capture the boot route once (the `ephemeralHub` pattern) and
   pass `enabled: false` to the hook.
3. **Invite and ephemeral gates collapse into the default.** The two
   existing effects are deleted; their behavior is a strict subset of
   the new one. `ephemeralHub` keeps suppressing the *error* gate (the
   preview must work without a set).
4. **`error` → a small retry card, not the setup form.** When the
   silent create (or a returning user's root connect) fails, show a
   minimal full-page card: "Couldn't connect to the sync server",
   the error text, a **Retry** button. `Retry` re-runs the hook's
   initialization (new `retry()` action on `useCollectionSets` that
   resets `initRef` and re-reads pointers). This is also an
   improvement for returning users: today a failed root connect shows
   the *fresh* setup form, whose "Create New Project Set" button would
   stack a second pointer.
5. **`ProjectSetSetup` is deleted** (tsx + css), along with its dev
   harness pages and their a11y-scan entries (all four modes; Q1).

## Design decisions

- **Hook, not a third copy of the effect.** `useAutoEstablishRoot`
  (`hub-client/src/hooks/useAutoEstablishRoot.ts`) takes
  `{ status, createProjectSet, migrateProjects, enabled }` and owns the
  fire-once ref. It replaces two effects and two refs in `App.tsx`,
  and is unit-testable with `renderHook` + mocked actions (the existing
  `useAuth.test.tsx` / `usePreference.test.tsx` show the pattern).
  The fire-once guard matters: `migrateProjects` resets status to
  `needs-migration` on failure, so an unguarded effect retry-loops
  against an unreachable server (comment in both existing copies).
- **`DEFAULT_SYNC_SERVER` is the only server.** The setup input was
  the one place a user could pick a different sync server at
  onboarding. Production, preview-embed and self-hosted builds all set
  `VITE_DEFAULT_SYNC_SERVER` at build time, which is the supported
  configuration channel; the input mostly served the e2e suite and
  local development. See Q2.
- **The e2e build points the default at the local hub.** Set
  `VITE_DEFAULT_SYNC_SERVER=/ws` on the `test:e2e` / `test:e2e:ui`
  scripts and in `.github/workflows/hub-client-e2e.yml`'s build env
  (next to `VITE_E2E: '1'`). `resolveSyncServerUrl` expands `/ws` to
  the page origin (`ws://localhost:5174/ws`), and `vite preview`
  already proxies `/ws` (with `ws: true`) to the hub that
  `globalSetup` starts on 3031 — the preview-embed build uses the
  identical mechanism. Projects seeded by `seedProjectInBrowser` keep
  their direct hub URL; only the root collection's server changes.
- **E2E bootstrap helpers wait instead of click.** `bootstrapProjectSetVariant`
  (and the two private copies) drop the fill/click and wait for the
  home to appear. The "Quarto Hub" heading assertion, which proved
  React had rendered before probing `__quartoTestReady`, becomes a
  `page.waitForFunction(() => '__quartoTestReady' in window)`; the
  classic variant still waits for the "Your Projects" heading and the
  collections variant for the search box.
- **No new `ProjectsHome` prop surface.** The retry card is a
  standalone component (`ProjectSetError.tsx`, ~40 lines, reusing the
  `.qh-error` / `.qh-btn` classes so it needs little or no new CSS);
  it does not route through `ProjectsHome`'s `error`/`onRetry`, which
  are about *project* connection failures and assume a connected set.

## Resolved questions (Carlos, 2026-09-15)

- **Q1 — Legacy migration.** (a): auto-migrate silently with
  `migrateProjects(DEFAULT_SYNC_SERVER)`, as the invite and ephemeral
  paths already do. The legacy IDB store is retained, so nothing is
  lost. `ProjectSetSetup` is deleted entirely (all four modes).
- **Q2 — Sync-server override.** None in the UI. The build-time
  `VITE_DEFAULT_SYNC_SERVER` is the only channel; which sync server a
  user is on is the deployment's responsibility, and users should never
  have to think about it. The retry card stays minimal.
- **Q3 — Inbound link after auto-create.** Accepted as-is: a browser
  that lands on the empty home and *then* opens a link-project-set URL
  gets the linked set as a secondary collection under an empty root.
  Rare, and worth the trade for the common first-signup path. Follow-up
  strand to file at ship prep ("replace the root when it is empty and
  unshared").
- **Q4 — Pre-existing link-route race.** Out of scope; filed as
  bd-88qrvqi4 (discovered-from bd-4h1hv60p).

## Work items

### Phase 1 — Tests first (red)

- [x] `hub-client/src/hooks/useAutoEstablishRoot.test.tsx`: fires
      `createProjectSet(DEFAULT_SYNC_SERVER)` once on `needs-setup`;
      fires `migrateProjects` once on `needs-migration` (per Q1); does
      nothing while `loading`/`connecting`/`connected`/`error`; does not
      re-fire when status returns to `needs-migration` after a failure;
      does nothing when `enabled` is false.
- [x] `useCollectionSets` `retry()` test (add to a new
      `useCollectionSets.test.tsx` or the reconciler test file, whichever
      already mocks `projectSetStorage`): after an `error`, `retry()`
      re-reads pointers and re-enters the state machine.
- [x] E2E: new `e2e/first-run.spec.ts` — fresh context, `goto('/')`,
      assert the setup card never appears and the collections home's
      "No projects yet" empty state does, then a project created through
      the UI lands in the root set (existing `projects-home.spec.ts`
      helpers). Also a classic-variant case waiting on "Your Projects".
- [x] E2E: update `bootstrapProjectSetVariant`, `import-zip.spec.ts`,
      `share-link-project-set.spec.ts` to wait rather than click
      (these go red as soon as the input disappears, which is the
      signal that the sweep is complete).
- [x] Harness: remove the `setup-*` rows from
      `e2e/baseline-a11y.harness.spec.ts`; grep `e2e/` for baseline
      artifacts keyed on those labels and remove them too.

### Phase 2 — Implementation

- [x] Add `useAutoEstablishRoot` hook; wire it in `App.tsx` with
      `enabled: bootRoute.type !== 'link-project-set'` (boot route
      captured once in `useState`, next to `ephemeralHub`).
- [x] Delete the invite (~L349) and ephemeral (~L451) effects and their
      refs/`eslint-disable` lines.
- [x] Add `retry` to `CollectionSetsActions`; reset `initRef` and re-run
      the init body (factor the init body into a `useCallback`).
- [x] Add `components/ProjectSetError.tsx`; replace both
      `ProjectSetSetup` render sites in `App.tsx` (the
      `needs-setup`/`needs-migration` gate goes away entirely;
      the `error` gate renders `ProjectSetError`, still skipped when
      `ephemeralHub`).
- [x] Delete `ProjectSetSetup.tsx` / `.css`; drop the DevHarness pages and
      import; update the `routing.ts` doc comment that names
      `setup-fresh`; update the `strings.ts` header comment.
- [x] `package.json` `test:e2e` / `test:e2e:ui` and
      `.github/workflows/hub-client-e2e.yml`: `VITE_DEFAULT_SYNC_SERVER=/ws`.
- [x] `npm run lint:css -w hub-client` (clean), `npm run build:all` in
      `hub-client/` (required before claiming done), `npm run test:ci`.

### Phase 3 — End-to-end verification

- [x] `npm run test:e2e` (full Playwright run against the local hub —
      the 31 specs that share the bootstrap helper are the regression
      net for this change).
- [x] Real browser: `cargo build --bin hub`, `npm run build:local-prod`,
      `npm run local-prod`; open `http://127.0.0.1:8080` in a fresh
      profile / after clearing site data; confirm sign-in → skeleton →
      "No projects yet" with no setup card; screenshot into this plan.
- [x] Real browser, failure path: same, with the hub stopped after the
      client is served (or `VITE_DEFAULT_SYNC_SERVER` pointed at a dead
      port): confirm the retry card, restart the hub, click **Retry**,
      confirm the home appears.
- [x] (covered by `first-run.spec.ts` → "a link-project-set boot URL makes the linked set the root"; not repeated by hand) Real browser, link path: from an established browser, copy "Link
      another browser…", open it in a fresh profile: the linked set is
      the root (no empty "My projects" beside it).

### Phase 4 — Ship prep

- [ ] `cargo xtask verify` (hub-client changed → full, not
      `--skip-hub-build`).
- [ ] Two-commit changelog workflow (`hub-client/changelog.md`, entry
      under `### 2026-09-15`).
- [x] File the Q3 follow-up strand (empty-root replacement on inbound link) with `discovered-from`. Q4 is already bd-88qrvqi4.
- [ ] Update this plan's checklist; close the strand after Carlos's
      review and merge.

## Notes

- `useProjectSet.ts` (the pre-collections singleton hook) still exists
  but `App.tsx` uses `useCollectionSets`; leave it alone.
- The `q2 preview --ui editor` embed relies on the ephemeral gate
  continuing to skip the *error* screen; keep that condition when
  swapping in `ProjectSetError`.
- bd-qhkp (stale closure on `projectSetState` in `handleProjectCreated`)
  is adjacent but unrelated; the new hook reads status through its
  own effect dependencies, so it does not inherit that bug.

## End-to-end verification record (2026-09-15)

Environment: worktree `.worktrees/bd-4h1hv60p-hub-client-auto-create`,
`cargo build --bin hub`, `npm run build:local-prod`, `npm run local-prod`
(hub on :3001, static+proxy on :8080), Chrome DevTools MCP driving
isolated browser contexts (fresh IndexedDB / localStorage each).

**Happy path.** Opened `http://127.0.0.1:8080/` in a fresh context. The
page went straight to the collections home's "No projects yet" empty
state — no setup card, `#setup-sync-server` absent (checked via
`document.querySelector`). IndexedDB `quarto-hub` → `projectSet` store
afterwards:

```json
[{"key":"collections","collections":[{"projectSetDocId":"23KX5jiVFqp2x1Act3NQCqvUszC6","syncServer":"ws://127.0.0.1:8080/ws"}]},
 {"key":"projectSet","projectSetDocId":"23KX5jiVFqp2x1Act3NQCqvUszC6","syncServer":"ws://127.0.0.1:8080/ws"}]
```

i.e. the root was created against the build's `DEFAULT_SYNC_SERVER` and
both pointers (array + legacy singleton) were written. Screenshot:
`hub-client/test-results/first-run-home.png` (gitignored), inspected.

**Failure path.** Second fresh context, navigated with an init script
that redirects every `WebSocket` to `ws://127.0.0.1:1/` while
`window.__wsBlock` is true. Rendered the retry card:

> Couldn't connect to the sync server
> Your project list lives on the sync server, and it did not answer.
> (Could not reach sync server. Please check your connection and try again.)
> [Try again]

Set `window.__wsBlock = false`, clicked **Try again**: the home's
"No projects yet" state appeared, root created. Screenshot:
`hub-client/test-results/first-run-retry-card.png`, inspected.

**Automated.** `npm run test:ci`: 103 + 17 + 23 files, 1,481 tests
passed. Playwright (`VITE_E2E=1 VITE_DEFAULT_SYNC_SERVER=/ws` build,
local e2e hub): 78 passed, including the 3 new `first-run.spec.ts` cases
(collections home, classic selector, link-project-set boot URL adopts
the linked set as root). Harness a11y baseline: 42 passed, with the new
`project-set-error` page scanned in both themes.
