# In-context debugging/diagnostic affordances for the hub-client editor SPA

**Strand:** bd-aim2gqis (parent)
**Phase strands:** bd-q93tkglb (1: am core) → bd-6ogrov5r (2: doctor + tap) →
bd-lb1cxprv (3: panel) → bd-09aja9gl (4: iframe). Phases 1→2→3 chained with
`blocks`; 4 is unblocked (cheap, anytime).
**Branch:** `braid/bd-q93tkglb-phase-1-quartodebugam-core` (off `main`)
**Status:** APPROVED 2026-07-30 — implementation in progress, Phase 1.

## Overview

`/debug.html` gives us a good view over Automerge data structures, but it runs
in a **separate browsing context** with its **own Repo** (ephemeral
server-connected, or read-only over IndexedDB). It can show what the *server*
or the *disk* believes; it cannot show what the *running editor* believes.
Bugs in the editor view — a Monaco buffer that diverged from its Automerge
doc, a file doc stuck in `requesting`, presence that stopped flowing, a stale
VFS — live in the SPA's JS heap, out of the debugger's reach.

Goal: debugging/diagnostic affordances that run **in the same JS context as
the React SPA**, serving two audiences:

1. **Humans** — a visual inspector over the live Automerge state, reachable
   from the editor without disturbing it.
2. **LLM agents** — machine-readable JS APIs (called via CDP
   `evaluate_script` / claude-in-chrome `javascript_tool`) that return small,
   JSON-serializable values so agents can navigate diagnosis without
   screenshots or scraping.

## Current state (survey, 2026-07-29)

### The two existing debug surfaces

- **`/debug.html`** → `src/debug/main.tsx` → `DebugApp`
  (plan: `2026-04-16-hub-client-automerge-debugger.md`). Separate Vite entry.
  Auth-gated (`useDebugAuthGate`). Two storage modes:
  - *Server (live)*: ephemeral `Repo` over `LoggingNetworkAdapter(WebSocketClientAdapter)`
    (`src/debug/services/repo.ts`, `LoggingNetworkAdapter.ts`) — logs every
    sync-protocol message into `MessageLog`.
  - *Local IndexedDB*: `ReadOnlyStorageAdapter(IndexedDBStorageAdapter)`, no
    network — inspects on-disk state without writes.

  Displays (`src/debug/components/`): `ConnectionStatus`, `DocumentList`
  (subscribe by URL), `DocumentViewer` (pretty-printed JSON dump per doc +
  index-doc detection with per-file subscribe buttons), `MessageLog`,
  `QuickPick`, `StoredLocalDocs`. All components consume the repo via
  `RepoContext.Provider` + `@automerge/automerge-repo-react-hooks` — i.e.
  they are already **parameterized over which Repo they inspect**. This is
  the key reuse seam.

- **`window.quartoDebug`** (`src/services/debugApi.ts`, bd-2rv8, plan
  `2026-05-01-hub-client-website-render-ux.md`). In-context, console/agent
  API at the *project/file/render* level: `project()`, `listFiles()`,
  `readFile()`, `writeFile()`, `rerender()`, `get/setActiveFile()`,
  `lastRenderResponse()`, `vfsList()`, `vfsRead()`. Installed from `App.tsx`
  behind `import.meta.env.DEV || localStorage.quartoDebug === '1'`, reads
  live state through refs. **It has no Automerge-layer affordances** — no doc
  ids, handles, heads, sync state, presence, or message traffic.

Also relevant: `window.__quartoTest` (`src/test-hooks.ts`, `VITE_E2E=1`
builds only) exposes `projectStorage`, `projectSet`, `wasmRenderer` for
Playwright; `DevHarness.tsx` + `#/dev/<page>` routes render isolated UI
states; `ReplayDrawer`/`useReplayMode` and `attribution-runs.ts` already walk
Automerge history for user-facing features.

### Where the live Automerge state actually lives

The SPA has **several Repos and repo-adjacent stores**, none currently
reachable from the console:

| State | Owner | Access today |
|---|---|---|
| Project index doc + per-file docs (text & binary) | `ts-packages/quarto-sync-client` `client.ts` (`state.repo`, `state.fileDocs`) | `getIndexHandle()`, `getFileHandle(path)`, `getSyncDiagnostics()` re-exported through `@quarto/preview-runtime/automergeSync.ts`; the **Repo itself is not exposed** |
| Project-set / collections docs | `src/services/projectSetService.ts` — module-level `servers: Map<url, {repo, wsAdapter}>`, one Repo per sync server | not exposed |
| Presence (ephemeral messages on the active file handle) | `src/services/presenceService.ts` | not exposed |
| Execution channel, attribution runs | `src/services/executionChannel.ts`, `attribution-runs.ts` | not exposed |
| WASM VFS | `@quarto/preview-runtime` | `quartoDebug.vfsList/vfsRead` ✅ |

`getSyncDiagnostics()` already returns `{connectedPeers,
unavailableRetryTicks, retryTimerActive, stranded[]}` — a prior investment in
exactly this direction, currently consumed by nothing user-visible.

### Why this is hard to do from /debug.html

A separate browsing context cannot share a `Repo` (JS heap objects don't
cross documents). The 2026-04-16 plan accepted that deliberately. Everything
below is about the complementary tool: same heap, live objects.

## Design space

Four building blocks, largely composable — this is not an either/or menu.
(A) is the foundation; (B)/(C) are alternative UI hosts; (D) feeds both.

### A. `quartoDebug.am` — machine-readable Automerge inspection API

Extend the existing `window.quartoDebug` with an `am` (automerge) namespace.
All results **plain JSON-serializable data** (snapshots, not live handles) so
they survive CDP serialization and are agent-friendly. Sketch:

```ts
interface QuartoDebugAutomerge {
  /** All repos known to this page, with peer/network summary. */
  repos(): { name: 'sync-client' | 'project-set'; syncServer: string;
             peerId: string; connectedPeers: string[] }[];

  /** Doc inventory: index doc, per-file docs, project-set docs. */
  docs(): { docId: string; role: 'index' | 'file' | 'binary-file' | 'project-set';
            path?: string; handleState: string; heads: string[] }[];

  /** Deep-cloned JSON snapshot of one doc (by docId or project path). */
  snapshot(ref: string): unknown;

  /** Heads + change-count + last-change metadata (actor, time, message). */
  history(ref: string, opts?: { limit?: number }): ChangeSummary[];

  /** getSyncDiagnostics() passthrough + per-doc handle states. */
  syncStatus(): SyncStatusReport;

  /** Current presence peers as seen by presenceService. */
  presence(): PresenceReport;

  /** Cross-checks that catch real divergence bugs:
      Monaco buffer vs automerge text, files map vs fileDocs cache,
      VFS listing vs project files. Returns a list of discrepancies. */
  doctor(): Discrepancy[];

  /** Escape hatch for interrogations without snapshot wrappers yet.
      Console use only; see settled decisions. */
  unsafe: { handle(ref: string): DocHandle<unknown>; Automerge: typeof A };
}

// Plus, at the top level: quartoDebug.help() — human+agent-readable usage
// document (the runtime contract) — and quartoDebug.apiVersion.
```

Notes:

- **`doctor()` is the agent-facing crown jewel**: one call that compares the
  layers (Automerge text ↔ Monaco model ↔ VFS ↔ files map ↔ handle states)
  and reports mismatches with enough context to pick the next probe. Most
  editor-view debugging sessions start with exactly these comparisons done
  by hand.
- Needs small upstream accessors: `quarto-sync-client` must expose its `Repo`
  (or enough of it: peerId, handle enumeration) and `preview-runtime` must
  re-export; `projectSetService` needs a `_debugServers()`-style accessor.
  All in-tree packages, low risk; accessors can be no-op-cheap and always
  compiled (they're just getters).
- Output-size discipline: `snapshot()` on a big doc can be huge. Truncate by
  default (`{maxBytes}` option, with an explicit `full: true` escape hatch)
  so agent transcripts don't blow up. Machine-readable = *small* by default.
- Testable with vitest (the existing `debugApi.test.ts` shows the pattern).

### B. Lazy-loaded in-context inspector panel (reuse `src/debug` displays)

A visual inspector mounted **inside the SPA**, over the **live repo**:

- Trigger: `quartoDebug.openInspector()` and/or a keybinding / `#/dev/inspector`
  route (DevHarness precedent). Closes with Esc / `closeInspector()`.
- Implementation: `React.lazy(() => import('./debug-panel/…'))` so the chunk
  is not in the main bundle; renders as a full-height drawer or overlay.
- Reuse: `DocumentViewer`, `DocumentList`, `MessageLog`, `QuickPick` already
  work against whatever `RepoContext.Provider` supplies. Mount them with the
  **sync-client's live Repo** and seed the doc list with the current
  project's index doc → the per-file subscribe UI works as-is. A small
  toolbar adds repo selection (sync-client vs project-set) and links each
  file doc to its project path.
- Live-context extras /debug.html can't have: current-file highlight, Monaco
  vs Automerge diff view (surfacing `doctor()` results visually), presence
  panel, sync diagnostics badge.
- Caution: components subscribe via `repo.find()` — on the *live* repo that
  is read-only in practice (`useDocument` doesn't mutate), but the panel must
  stay observation-only by convention; we're pointing UI at production
  handles. No write affordances in the panel.

### C. Dynamically-injected iframe hosting `/debug.html`

`quartoDebug.openServerInspector()` injects `<iframe src="./debug.html#doc=<indexDocUrl>">`
(the one-shot hash seed already exists in `DebugApp`).

- What it buys: **zero new UI code**, and — because the iframe deliberately
  keeps its own server-connected Repo — a **live-vs-server comparison**: the
  editor's in-memory heads (via A) next to the server's view of the same doc
  (iframe). Divergence between those two is precisely what several sync bugs
  look like.
- What it doesn't buy: the iframe still can't see the SPA heap. It is a
  *companion*, not a replacement for B. A `postMessage` bridge (iframe asks
  parent for `quartoDebug.am.*` snapshots) could later merge the two views,
  but that's scope creep for a first pass.
- Cheap enough to ship alongside either A or B.

### D. Live sync-message tap (ring buffer)

/debug.html's `MessageLog` works because its adapter is wrapped at
construction. To observe the **editor's own** sync traffic:

- Add an optional network-adapter wrap hook (or diagnostics event emitter) to
  `quarto-sync-client`'s repo construction; reuse `LoggingNetworkAdapter`
  (move it from `src/debug/services/` to a shared location).
- Ring buffer (e.g. last 500 messages, payloads summarized: type, docId,
  byte-size, timestamp) exposed as `quartoDebug.am.messages()` and rendered
  in panel B via the existing `MessageLog` component.
- Gate at connect time behind the same debug flag; when disabled the wrap is
  skipped entirely (zero overhead). Enabling therefore takes effect on the
  next project connect — acceptable; document it.

## Recommended shape

Phased, each phase independently useful; stop points between all of them:

1. **Phase 1 — `quartoDebug.am` core (option A minus doctor):** upstream
   accessors in `quarto-sync-client` / `preview-runtime` /
   `projectSetService`; `repos/docs/snapshot/history/syncStatus/presence`;
   vitest coverage; agent-usage notes in the API doc comments.
2. **Phase 2 — `doctor()` + message tap (A finish + D):** cross-layer
   discrepancy checks; adapter tap + `messages()`.
3. **Phase 3 — in-context inspector panel (B):** lazy chunk, live-Repo
   RepoContext mount of the existing debug components, plus
   presence/diagnostics/doctor panes.
4. **Phase 4 (cheap, anytime) — iframe embed (C):** `openServerInspector()`
   with hash-seeded doc.

Rationale for A-first: it's the substrate (B renders what A computes; agents
get value immediately); it has the smallest blast radius; and it forces the
accessor plumbing that every other option needs anyway.

## Cross-cutting decisions (settled 2026-07-30 with Carlos)

- **Gating: SETTLED.** Single gate everywhere, the existing one (`DEV ||
  localStorage.quartoDebug === '1'`), and it ships in prod builds behind the
  flag — we explicitly *want* early users to be able to flip it on to help
  with debugging. The panel is a lazy chunk that never loads unless invoked.
  More sophisticated gating may come later; not now.
- **Unsafe escape hatch: SETTLED (yes, shaped as a namespace).**
  `quartoDebug.am.unsafe` with two members: `handle(ref)` → live `DocHandle`,
  and `Automerge` → the automerge module itself. The module export is what
  makes the handle useful from a console (no `import` there): it unlocks
  `getConflicts()`, `diff(doc, headsA, headsB)`, `view(doc, oldHeads)`
  time-travel, and change forensics (`getAllChanges`/`decodeChange`) — none
  of which have snapshot wrappers yet. The handle alone adds untruncated
  live traversal (`doc()`) and live event subscription (`on('change')`).
  This is a stopgap for interrogations we haven't wrapped; any use that
  becomes routine graduates to a first-class snapshot API (`conflicts()`,
  `diff()`, `watch()`). `help()` carries a one-line warning that
  `handle.change()` bypasses sync-client invariants (file-doc caches, VFS
  mirroring, Monaco sync) — observation only.
- **Read-only guarantee: SETTLED.** The API observes; mutations stay in the
  existing file-level `writeFile()`. No Automerge-level mutation affordances
  (`change()`, merge, fork) in v1 (the unsafe hatch is not an endorsement).
- **Documentation & discovery: SETTLED.** `quartoDebug.help()` returns a
  compact usage document written to be readable by humans *and* agents —
  method inventory, result-shape sketches, the read-only rule, and the
  unsafe warning. It is the runtime contract and must be updated in the same
  commit as any API change (add a test that `help()` mentions every key of
  `quartoDebug` and `quartoDebug.am` so it can't silently rot). Doc comments
  in `debugApi.ts` remain the detailed reference. Add `quartoDebug.apiVersion`.
- **MessageLog payload retention (D): SETTLED.** Summaries by default
  (type, docId, byte-size, timestamp); full payloads via a params object at
  enable time (`{capture: 'full'}`). The params-object shape is deliberate:
  it leaves room to evolve capture behavior without breaking callers.

## Test strategy sketch (per project TDD rules)

- Phase 1: vitest unit tests per `am.*` method against a mocked/fixture
  sync-client (pattern: `debugApi.test.ts`); wasm-tagged tests where VFS
  comparison is involved.
- Phase 2: `doctor()` tests that *manufacture* each discrepancy class
  (Monaco/Automerge divergence via direct model edit, stranded file doc) and
  assert detection; adapter-tap tests reuse `LoggingNetworkAdapter.test.ts`.
- Phase 3: component tests mounting the panel with a test Repo; one
  Playwright E2E driving `openInspector()` in a real session.
- End-to-end verification (non-negotiable): a real browser session against
  `npm run dev`, exercising each API from the console and the panel over a
  real project; transcript to include invocations + observed output.

## Work items

Design iteration:

- [x] Survey current source (debug.html, debugApi, sync-client, services)
- [x] Strand bd-aim2gqis created and linked to this plan
- [x] Settle cross-cutting decisions (2026-07-30; see section above)
- [x] Phasing confirmed (A → D → B → C); child strands cut (bd-q93tkglb,
      bd-6ogrov5r, bd-lb1cxprv, bd-09aja9gl); go-ahead received 2026-07-30

### Phase 1 — `quartoDebug.am` core (bd-q93tkglb)

Upstream accessors (each with tests first):

- [x] `quarto-sync-client`: `getRepo()` + `getDocInventory()`
      (`DocInventoryEntry`: docId/role/path/handleState/heads/
      unavailableMarker; index first, then by path). Tests:
      `src/doc-inventory.test.ts` against the real test-hub (6 tests,
      incl. heads-advance-on-edit and dangling-entry cases)
- [x] `preview-runtime` (`automergeSync.ts`): null-safe re-exports
      (`getRepo` → null, `getDocInventory` → [] before connect); also
      re-exported the `SyncDiagnostics`/`DocInventoryEntry` types from
      the barrel; mockSyncClient extended
- [x] `projectSetService`: `getProjectSetDebugSnapshot()` via pure
      `buildProjectSetDebugSnapshot()` (tested against fabricated
      repos/handles) + `getCollectionHandle(docId)`
- [x] `presenceService`: `getPresenceDebugSnapshot()` (returns copies;
      probe-safe pre-init; identity is the userId/userName/userColor
      projection)

`am` namespace (`hub-client/src/services/debugAutomerge.ts`, TDD per method —
23 tests in `debugAutomerge.test.ts`):

- [x] `repos()` — sync-client repo (syncServer from ctx) + project-set servers
- [x] `docs()` — sync-client inventory merged with `project-set`-role entries
- [x] `snapshot(ref, opts)` — refs: path | 'index' | docId (bare or prefixed);
      default truncation (strings >500 chars, depth >12) with `truncated`
      flag; `{full: true}` escape; `Uint8Array` always summarized as
      `{$type:'bytes', length}` (TDD caught a real eval-order bug where
      `truncated` was read before the sanitizer ran)
- [x] `history(ref, opts)` — `DocHandle.history()`/`metadata()`, newest
      first, `limit` default 20, includes actor/timestamp/message
- [x] `syncStatus()` — connected + diagnostics (null when no client) +
      project-set snapshot
- [x] `presence()` — passthrough of the presence snapshot
- [x] `unsafe.handle(ref)` + `unsafe.Automerge` (module identity)
- [x] `help()` + `apiVersion` (= 1) + test that `help()` covers every key of
      the API, of `am`, and of `am.unsafe`, plus the unsafe warning
- [x] Wire into `installDebugApi` — no `App.tsx` change needed; `am` is fed
      from the existing `DebugApiContext.getProject`
- [ ] `npm run build:all` + `test:ci` green (build running)
- [ ] End-to-end: real browser session, exercise each method from console,
      record invocations + output here
- [ ] Close bd-q93tkglb

### Phase 2 — `doctor()` + message tap (bd-6ogrov5r)

- [ ] Checklist to be detailed at phase start

### Phase 3 — inspector panel (bd-lb1cxprv)

- [ ] Checklist to be detailed at phase start

### Phase 4 — iframe embed (bd-09aja9gl)

- [ ] Checklist to be detailed at phase start

## References

- `claude-notes/plans/2026-04-16-hub-client-automerge-debugger.md` — /debug.html port
- `claude-notes/plans/2026-05-01-hub-client-website-render-ux.md` — bd-2rv8, `window.quartoDebug`
- `hub-client/src/services/debugApi.ts` — existing console API
- `hub-client/src/debug/` — reusable inspector components/services
- `ts-packages/quarto-sync-client/src/client.ts` — Repo owner; `getSyncDiagnostics()`
- `hub-client/src/services/projectSetService.ts` — collections Repos
