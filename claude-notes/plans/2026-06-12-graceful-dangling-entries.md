# bd-vm5e5u10: one dangling index entry must not brick a project

**Strand:** bd-vm5e5u10 (p1). Related: bd-10deu8h4 (the creator bug —
how dangling entries get minted), bd-8x482xb0 (closed — the production
casualty), bd-p68lx71t (the 2026-06-12 incident this amplified),
bd-10bdjmjb (parent plan:
`claude-notes/plans/2026-06-12-sync-client-offline-race.md`).
**Status:** READY TO IMPLEMENT — design agreed with Carlos in the
2026-06-12 session; written as a self-contained handoff.

## Branch / coordination — READ FIRST

The defect lives in `ts-packages/quarto-sync-client`, which received
several fixes on the **`beads/bd-81cfshmw-q2-mcp-launcher`** branch
(requireOnline/PeerUnavailableError, never-throw ws error handlers,
the test-hub harness, offline-creation regression tests). That branch
is NOT yet merged to main; it is pushed as
**`origin/feature/bd-81cfshmw-q2-mcp-launcher`** (repo convention:
local `beads/…` names map to remote `feature/…` — see
`.claude/rules/worktrees.md` § Pushing for PR). **Branch off that
remote ref** (or off main after it merges) —
implementing against plain main will conflict and will lack the test
harness this plan tells you to use. `cargo xtask switch-task
bd-vm5e5u10 --from beads/bd-81cfshmw-q2-mcp-launcher` is the
sanctioned way (see `.claude/rules/worktrees.md`).

## Parallel work notice

**bd-10deu8h4** (MCP exit-sync drain, plan
`2026-06-12-mcp-exit-sync-drain.md`) is being implemented in parallel
on the same integration line. Boundary contract: THEY own
`client.ts` `disconnect()` + drain primitives, `quarto-hub-mcp`
`index.ts` shutdown and `connection-manager.ts` `disconnectAll`; YOU
own `loadFileDocuments` / `syncWithFiles` / `indexChangeHandler` and
all of `tools.ts`. Shared additive-only: `types.ts`, sync-client
`index.ts` exports, new test files. Second merger resolves.

## What happens today (the defect)

A project's index document maps paths → automerge doc ids. If one
referenced document does not exist on the hub (a "dangling entry"),
**every client fails the entire project**:

- `ts-packages/quarto-sync-client/src/client.ts`
  - `loadFileDocuments` (~line 434): `for` loop, `await findDoc(...)`
    per file, **no try/catch** — the first unavailable doc throws out
    of `connect()`. Cold opens of the project fail entirely.
  - `syncWithFiles` (just below, ~line 448): same pattern in the
    **index-change handler** — sessions with the project already open
    blow up when a dangling entry *appears* in the index (this is how
    already-open colleagues got hit on 2026-06-12).
  - `findDoc` (~line 332): retries "unavailable" up to 3× (with an
    early bail when `connectedPeers.size === 0` — see Gotchas), then
    throws `Document <id> is unavailable`.
- Consumers of `connect()` that therefore hard-fail:
  - hub-client browser SPA (via `@quarto/preview-runtime`
    `automergeSync.connect`) — "failures to open project";
  - the MCP server: `connect_project` returns
    `Error in connect_project: Document automerge:… is unavailable`
    (ts-packages/quarto-hub-mcp/src/connection-manager.ts `connect`).

Production evidence (2026-06-12): one entry
(`/cscheid/q2-mcp-hello.qmd` → a doc that never reached the hub)
made the Demo Playground unopenable for every browser and the MCP
alike, surfacing as an incident. The entry was surgically removed;
**the test fixture no longer exists in production — tests must mint
their own dangling entries** (trivial, see Test plan).

## Required behavior (the fix)

1. `connect()` succeeds if the **index** loads, even when some file
   documents are unavailable. Unavailable files are skipped by the
   loading loop, reported (see 3), and do not affect other files.
2. `syncWithFiles` (index-change path) gets the same tolerance: a
   dangling entry appearing mid-session must not throw / reject
   unhandled; available files continue to work.
3. Unavailability is **surfaced, not swallowed**:
   - extend the per-file information returned by `connect()` and kept
     in client state with an availability marker. `FileEntry` (from
     `@quarto/quarto-automerge-schema`) is `{ path, type }` today —
     prefer adding `status?: 'ok' | 'unavailable'` at the
     sync-client boundary rather than changing the schema package
     (the index document's wire format is NOT changing; this is
     client-side presentation. If you do touch the schema package,
     check its other consumers: hub-client, preview-runtime, wasm
     tests).
   - new optional callback `onFileUnavailable?(path: string, docId:
     string)` in `SyncClientCallbacks`
     (ts-packages/quarto-sync-client/src/types.ts) so UIs can show a
     degraded marker. All existing callbacks/consumers must keep
     compiling without changes (it's optional).
4. The **index document remaining unavailable stays fatal** — nothing
   sensible can be done without it; keep that error path, but improve
   its message (see 5).
5. **Error-message clarity** (Carlos was misled by the current text
   during the incident; users will be too): anywhere the client or
   MCP surfaces unavailability, name what kind of document and which
   path: e.g. `file document for '/cscheid/x.qmd'
   (automerge:3HJo…) is unavailable on the sync server — the file may
   have been created by a client that never synced it` vs `project
   index document <id> is unavailable`.
6. MCP server behavior
   (ts-packages/quarto-hub-mcp/src/tools.ts):
   - `connect_project` / `list_files`: succeed, listing unavailable
     files with `"status": "unavailable"` in the JSON payload.
   - `read_file` / `write_file` / `patch_file` on an unavailable
     file: clear per-file error (per 5), project stays connected.
   - `delete_file` / `rename_file` on an unavailable file: these only
     edit the index — **deleting a dangling entry must work** (that
     is the self-service repair story; the 2026-06-12 surgery would
     have been `delete_file` if this had existed). Check
     `handleDeleteFile`/`handleRenameFile` paths for any doc-load
     dependency and remove it for the delete case.
7. Out of scope (do NOT do here): retrying unavailable files when the
   peer connects later (that's D2, parent plan Phase 3); creating the
   repair/doctor tooling (parent plan Phase 1.5); changing the 1 ms
   peer-wait default (D3).

## Test plan (TDD — red first, per CLAUDE.md)

Harness: `ts-packages/quarto-sync-client/src/test-hub.ts` (in-process
automerge-repo hub with `hubHasDoc`, `holdUpgrades`) — already on the
branch. Minting a dangling entry in tests: create a project normally
through a sync client, then mutate the index through the hub's own
repo handle:

```ts
const handle = await hub.repo.find(indexDocId);
handle.change((d) => { d.files['/ghost.qmd'] = 'Av7qtCPQVkStggRLxomA2vW728U'; }); // unknown doc id
```

New file `ts-packages/quarto-sync-client/src/dangling-entries.test.ts`:
1. **connect succeeds with a dangling entry** — project with 2 real
   files + 1 minted ghost: `connect()` resolves; returned entries
   include both real files (`status: 'ok'` or marker absent) and the
   ghost with `status: 'unavailable'`; `onFileAdded` fired for real
   files only; `onFileUnavailable` fired for the ghost;
   `getFileContent` works for real files. (RED today: connect
   throws.)
2. **dangling entry appearing mid-session** — client connected and
   online; mint the ghost via the hub repo; assert no unhandled
   rejection (vitest fails on them) and `onFileUnavailable` fires;
   existing files still readable/editable. (RED today.)
3. **index unavailable stays fatal** — connect to a nonexistent index
   doc id with `requireOnline: true`: still rejects, message contains
   "index" and the id. (Probably green today except message text —
   lock the improved message.)
4. **error message content** — assert the unavailable-file error
   names path + doc id + "sync server" (whatever exact wording you
   choose in 5; lock it).

MCP level, extend `ts-packages/quarto-hub-mcp/src/` (suite already
spawns the dist server over stdio — see `stdio-hygiene.test.ts` for
the pattern; `McpTestClient` + its own `test-hub.ts`; you may need to
add the index-mutation helper there too, or export it):
5. `connect_project` on a ghosted project succeeds and the JSON
   marks the ghost `"status": "unavailable"` (RED today: tool errors).
6. `read_file` of the ghost → error naming the path; subsequent
   `read_file` of a real file in the same session works.
7. `delete_file` of the ghost succeeds and removes the index entry
   (assert via `list_files`).

Build order for running TS tests (workspace deps resolve via dist):
`npm run build` in `ts-packages/quarto-automerge-schema`, then
`quarto-sync-client`, then `quarto-hub-mcp`; then `npx vitest run` in
the package. Full gate before calling it done:
`cargo xtask verify --skip-hub-build --skip-hub-tests` (runs the
hub-mcp/sync-client suites as Step 11) **plus** `cd hub-client && npm
run build && npm run test:ci` (sync-client is a hub-client dep;
type/interface changes can break its production build — `build:all`
not needed unless you touch Rust/wasm-affecting code, which this work
should not).

## Implementation sketch

In `client.ts`:
- Wrap the per-file body of `loadFileDocuments` and `syncWithFiles`
  in try/catch; on an `unavailable`-matching error: record
  `state.unavailableFiles.set(path, docId)` (new map alongside
  `fileHandles`), fire `callbacks.onFileUnavailable?.(path, docId)`,
  `syncLog` a diagnostic (NOT console.log — bd-sl4o01y0), continue.
  Non-unavailable errors keep throwing (don't mask real bugs).
- `connect()`'s returned `FileEntry[]`: annotate from
  `state.unavailableFiles`.
- Clear `unavailableFiles` appropriately on disconnect and when an
  entry is removed from the index (`syncWithFiles` removal branch).
- Keep `findDoc` itself unchanged (its retry semantics are D2/D3
  territory; you only need its thrown error to be recognizable —
  it already matches `/unavailable/i`).

In `tools.ts` (hub-mcp): thread the status through
`handleConnectProject`/`handleListFiles` JSON; per-file guards in
read/write/patch with the clear message; ensure delete/rename of an
unavailable path skips any doc fetch.

## Gotchas, from the people who got got

- **Stdout purity**: never `console.log` in sync-client — use
  `syncLog` (`src/log.ts`); there's a source-level invariant test
  that will fail your PR otherwise (bd-sl4o01y0).
- **The connect spy**: `quarto-hub-mcp/src/connection-manager.test.ts`
  has a `spySyncClientFactory` modeling `connect`'s signature — if
  you change the connect return shape, update it (it bit us once:
  options-bag migration).
- **`findDoc`'s `connectedPeers.size === 0` bail** (e326eb5c): noted
  latent cold-boot issue, deliberately NOT in scope — don't "fix" it
  in passing; it's tracked in the parent plan's D2 with its own test
  requirements.
- **vitest + unhandled rejections — CONFIRMED**: the index-change
  handler calls `syncWithFiles(newFiles)` fire-and-forget (no await,
  no void; `client.ts` ~578), so today's mid-session failure is an
  *unhandled promise rejection*. Your fix should make that call
  explicitly handled (`void syncWithFiles(...).catch(...)` routing
  into the same unavailable-tolerance), and test 2 should assert no
  unhandled rejection escapes (vitest fails runs on them by
  default — rely on that, plus the onFileUnavailable assertion).
- **Don't pipe test runs through `tail`/`grep` when exit codes
  matter** — a swallowed vitest failure produced a false green in
  this session. Run plainly or capture to a file and check `$?`.
- Tests gated on Rust binaries follow the pattern in
  `offline-creation-rust-hub.test.ts` (`describe.runIf` + loud skip
  message); for THIS work the in-process JS hub suffices — no Rust
  binary needed.
- macOS-only validation is acceptable for now (Carlos, 2026-06-11);
  CI parity for Windows comes later via review.

## Acceptance criteria

- [ ] All 7 new tests green; every pre-existing suite green
      (sync-client, hub-mcp incl. bundle + e2e-auth where gated,
      hub-client build + test:ci, `cargo xtask verify
      --skip-hub-build --skip-hub-tests`).
- [ ] Manual e2e per CLAUDE.md (binary-level, not just vitest): local
      hub + `q2 mcp` (rebuild bundle + binary first: `cargo xtask
      build-hub-mcp-bundle && cargo build --bin q2`); mint a ghost
      entry; `connect_project` lists it as unavailable; `delete_file`
      repairs it; record the transcript in this plan.
- [ ] Error messages reviewed against requirement 5 (a human can tell
      file-vs-index and which path).
- [ ] braid: comment + close bd-vm5e5u10 with commit hash; note in
      parent plan (`2026-06-12-sync-client-offline-race.md`) that the
      amplifier is fixed.
- [ ] Do NOT deploy to quarto-hub.com from this strand; rollout is
      coordinated in the parent plan (Phase 5) and the deployment
      repo currently has standing notes in bd-erf there.
