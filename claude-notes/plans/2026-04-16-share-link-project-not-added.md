# Share link project never joins the synced project set

## Symptom

1. Visiting a share link `#/share/<indexDocId>?server=...&file=...&name=...` opens and edits the project just fine.
2. The project never appears in the user's project list on the landing page (the list driven by the synced profile / project set).
3. Trying to re-add the same `indexDocId` via the "Connect to Project" form on the landing page fails with:

   > Failed to add project. The document ID may already exist.

   even though the `indexDocId` is **not** present in the profile's `projects` map (confirmed via `/debug.html`).

Reproducible example from the reporting user:
- share URL: `https://quarto-hub.com/#/share/SNHcgVzUkWpGFmcxkCkpCDfFtmu?server=wss%3A%2F%2Fquarto-hub.com%2Fws&file=%2Felliot%2Findex.qmd&name=Demo+Playground`
- `SNHcgVzUkWpGFmcxkCkpCDfFtmu` is NOT in the user's profile `projects` map.

## Root cause (three separate bugs that combine)

### Bug A — Race in the share-route handler

`hub-client/src/App.tsx:229-289` handles `#/share/...` on mount. It:

1. Writes the project to IndexedDB (`projectStorage.addProject`, lines 250-257).
2. Adds the project to the synced project set, **but only when the project-set connection is already `connected`** (line 260):

   ```ts
   if (projectSetStateRef.current.status === 'connected') {
     try {
       projectSetActions.addProject({ ... });
     } catch {
       // Non-fatal: project set update failed, but project is in IDB
     }
   }
   ```

The share-route effect runs once at mount. The project-set connection is initialised by `useProjectSet` (`hub-client/src/hooks/useProjectSet.ts:86-121`), which goes `loading → connecting → connected` asynchronously over at least one websocket round-trip. On initial page load (the whole point of opening a share link) the status is overwhelmingly `loading` or `connecting` when the share handler runs, so the guard fails, the `addProject` call is skipped, and no retry ever happens.

Net effect: the project lands in IDB but never in the synced profile doc.

### Bug B — Project list is driven by the project set, not IDB

`hub-client/src/components/ProjectSelector.tsx:126-134`:

```ts
const projectSetConnecting = projectSetStatus === 'loading' || projectSetStatus === 'connecting';
const useProjectSet = !!projectSetEntries || projectSetConnecting;

const loadProjects = useCallback(async () => {
  if (useProjectSet) {
    // Projects come from the project set — no need to load from IDB
    setLoading(false);
    return;
  }
  ...
}, [useProjectSet]);
```

Any user whose browser has ever connected to a project set (i.e. essentially every existing user) renders the list from `projectSetEntries`. So even though Bug A left the project in IDB, the landing page never shows it. This is not itself a bug, but it's why Bug A's silent skip produces an *invisible* project.

### Bug C — "Connect to Project" form skips the duplicate check and hits IDB's unique index

`hub-client/src/components/ProjectSelector.tsx:265-301` (`handleConnectProject`):

```ts
let normalizedDocId = indexDocId.trim();
if (!normalizedDocId.startsWith('automerge:')) {
  normalizedDocId = `automerge:${normalizedDocId}`;
}

const project = await projectStorage.addProject(
  normalizedDocId,
  syncServer.trim(),
  description.trim() || undefined
);
```

Compare with the share route, which calls `projectStorage.getProjectByIndexDocId` first and only inserts when missing. This form doesn't. Every call is a fresh `db.put` with a freshly generated primary key.

IndexedDB has a **unique** index on `indexDocId` (`hub-client/src/services/storage/db.ts:38`):

```ts
store.createIndex('indexDocId', 'indexDocId', { unique: true });
```

So when the user tries to re-add the same document the share route just wrote, `db.put` throws a `ConstraintError` and the catch at line 297 produces the misleading message:

```ts
catch (err) {
  console.error('Failed to add project:', err);
  setFormError('Failed to add project. The document ID may already exist.');
}
```

The message is *technically* true — the row exists in IDB — but it is deeply misleading because (a) the duplicate was created by the app itself on the previous page load, and (b) the project genuinely is **not** in the profile the user is looking at.

### Additional problem: Connect form never touches the project set either

Even if Bug C were fixed (e.g. dedupe on IDB), `handleConnectProject` doesn't call `projectSetActions.addProject`. So the form as written cannot repair the state that Bug A leaves behind: the only way the user could get the project into their visible list is by leaving the landing page, opening the share link again, and hoping the project set happens to be `connected` that time.

## Repro confirmed

E2E spec at `hub-client/e2e/share-link-project-set.spec.ts` reproduces both failures in under 10s:

- `share link adds the project to the receiver's synced project set` — currently `test.fail()`, failing at the final assertion (Bug A + B).
- `after visiting a share link, re-adding the same doc via Connect form is idempotent` — currently `test.fail()`, failing at the "may already exist" toast assertion (Bug C).

Run with: `cd hub-client && npx playwright test share-link-project-set.spec.ts`. Flip `test.fail` → `test` once the fix lands.

## Fix plan (TDD) — reconciler approach

We choose a reconciler over "enqueue-and-flush" because:
- It is a pure `(idbProjects, setProjects) → adds` function, trivially unit-testable with no timing mocks.
- It self-heals users already in the broken state (anyone who hit Bug A has orphan IDB rows).
- It removes the timing dependency entirely from the share route and the Connect form — both can be "naive writers" and the reconciler is the single place that keeps IDB ⊆ project set.

### Phase 1 — Tests first

- [x] Unit test: pure `computeReconcileAdds(idbProjects, setEntries)` returns the list of entries to add (those in IDB but not in the set, keyed after stripping `automerge:`). Covers: empty inputs, exact match, missing entry, prefix-normalisation collision, duplicate IDB rows with same indexDocId. — `hub-client/src/services/projectSetReconciler.test.ts` (9 tests, green).
- [x] E2E: `reconciler adopts an orphan IDB entry into the connected project set` — seeds IDB directly to simulate the post-Bug-A state, asserts the reconciler catches it on the next `connected` tick. Verified red (fails without the useProjectSet wiring) → green (passes with it). This replaces a mock-handle unit test — it exercises the real React effect and the real project-set service.
- [x] E2E: `share link adds the project to the receiver's synced project set` — end-state check for the share-flow scenario. Note: on localhost this passes even on broken code because the peer connects fast enough to beat the share handler's `status === 'connected'` check; it's a green-guard, not a hard Bug-A repro. The reconciler test above is the deterministic Bug-A repro.
- [x] E2E: `after visiting a share link, re-adding the same doc via Connect form is idempotent` — hard red/green indicator for Bug C. Fails on unfixed code with the literal "may already exist" toast.
- [ ] ~~Share-route regression test~~ — not worth a dedicated test; the existing specs all exercise the share route and would fail on any regression there.

### Phase 2 — Implementation

- [x] Add a pure `computeReconcileAdds` function and a thin `reconcileIntoConnectedProjectSet` wrapper that wires it to the live services. Lives in `hub-client/src/services/projectSetReconciler.ts`.
- [x] Wire the reconciler into `useProjectSet`: a `useEffect` that watches `status` and, whenever it is `connected`, runs the reconciler and refreshes the React state if anything was added. Lives in `hub-client/src/hooks/useProjectSet.ts` (around the old init effect).
- [x] `handleConnectProject`: dedupe against IDB via `getProjectByIndexDocId` before inserting; when `useProjectSet` is truthy, also call the new `onAddProjectToSet` prop. Plumbed through `ProjectSelector`'s `Props` and wired up in `App.tsx` to `projectSetActions.addProject`.
- [x] Replace the blanket "may already exist" error with `err.message` (when available) or a generic "Failed to add project." since the happy path no longer hits the unique-index collision.
- [x] Leave the share-route's one-shot `projectSetActions.addProject` call in place — harmless fast path, but no longer load-bearing because the reconciler handles the race miss.

### Phase 3 — Verification

- [x] All new unit tests pass — 9/9 in `projectSetReconciler.test.ts`.
- [x] Flipped `test.fail` → `test` in `hub-client/e2e/share-link-project-set.spec.ts`. All three scenarios pass in 5.4s (`npx playwright test share-link-project-set.spec.ts`).
- [x] `cd hub-client && npm run build:all` — production build succeeds (9.86s).
- [x] `cd hub-client && npm run test:ci` — full hub-client test suite passes (52 tests).
- [ ] ~~`cargo xtask verify`~~ — skipped. No Rust files changed in this fix.
- [ ] Manual smoke on a real environment (`quarto-hub.com`) — deferred until deployed.

## Open questions

- Should the share route also `projectSet.touchProject` when the project is already present? Currently it doesn't, which means share-link visits don't bubble the project up in the recency sort on other browsers.
- Is it worth tightening the IDB schema — dropping the `unique: true` constraint and relying on `getProjectByIndexDocId` — or upserting by the natural key? Probably out of scope here; flagged for a separate issue if desired.
