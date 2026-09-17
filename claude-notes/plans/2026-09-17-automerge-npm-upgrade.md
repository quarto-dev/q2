# Upgrade `@automerge/automerge` and `@automerge/automerge-repo` (npm)

**Strand:** bd-d08gpqvu
**Branch:** `braid/bd-d08gpqvu-automerge-npm-upgrade` (worktree under `.worktrees/`)
**Date:** 2026-09-17

## Overview

Survey of the automerge npm packages across the npm workspace and the
upgrade that follows from it. Four workspace packages depend on the
automerge stack: `hub-client`, `ts-packages/quarto-sync-client`,
`ts-packages/quarto-hub-mcp`, and `ts-packages/preview-runtime`.
`resources/extension-build/deno.json` also lists them (mirrored in the
root `deno.lock`).

### Survey (2026-09-17)

| package | installed | newest stable | npm `latest` tag | npm `next` tag |
| --- | --- | --- | --- | --- |
| `@automerge/automerge` | 3.4.1 | **3.5.0** (2026-09-16) | 3.5.0 | 3.4.0-rev-frag-hex.3 |
| `@automerge/automerge-repo` (+ `-network-websocket`, `-react-hooks`, `-storage-indexeddb`, `-storage-nodefs`) | 2.5.6 | **2.5.6** (2026-05-18) | 2.6.0-alpha.3 | 2.6.0-alpha.5 (2026-09-16) |

Observations:

- `@automerge/automerge` 3.4.1 → 3.5.0 is a plain stable minor. Release
  notes: opt-in change "author" metadata (`getAuthor`/`getAuthors`),
  `__proto__` assignment now throws, large-list insertion stack-overflow
  fix, `applyPatch(es)` block/mark fixes, a perf regression fix, a list
  insertion positioning fix. `automerge-repo@2.5.6` declares
  `@automerge/automerge: "2.2.8 - 3"`, so 3.5.0 satisfies it with one copy
  in the tree.
- The automerge-repo family has **no stable release newer than 2.5.6**.
  Upstream has shipped only `2.6.0-alpha.{0,1,2,3,5}` since May, and
  moved the npm `latest` dist-tag onto `2.6.0-alpha.3`. The alpha line
  removes xstate (DocHandle state machine rewritten, PR 618), adds
  `DocumentQuery`, rewrites subdoc handles, raises `engines.node` to
  `>=22.13`, bumps `uuid` to 14 / `bs58check` to 4, and carries a long
  list of robustness fixes (throttle, storage, websocket teardown,
  react-hooks stale results, ephemeral-message delivery in alpha.5).
- Version-range drift inside the workspace: `quarto-sync-client` and
  `preview-runtime` still declared `^2.5.1` / `^2.5.4` for the repo
  packages while `hub-client` / `quarto-hub-mcp` declared `^2.5.6`. The
  lockfile already resolved all of them to 2.5.6; only the declared
  ranges differed.
- `hub-client` pins `-react-hooks` and `-storage-indexeddb` **exactly**
  (`2.5.6`, no caret). That pin dates from commit `083814400f` ("Fix deps
  hopefully", 2026-06-09) with no recorded rationale; it predates that
  commit too (`2.5.1` exact). Kept as-is in this pass — it is harmless
  while the whole family sits on one version.
- Rust side, for context only (out of scope here): the hub uses
  `samod` 0.13.0 from the `quarto-dev/samod` fork (`access-policy`
  branch) with Rust `automerge` 0.11.0. Upstream `samod` 0.14.0 landed on
  crates.io on 2026-09-17. The JS 3.5.0 "author" metadata is opt-in, so
  JS clients on 3.5.0 keep producing changes the 0.11 Rust reader
  understands unless we start calling the new API.

## Decision: stable-only (phase 1) vs. 2.6.0 alpha (phase 2)

Phase 1 is unambiguous: take the stable `@automerge/automerge` minor and
align the drifted ranges. Phase 2 (adopting a 2.6.0 alpha because
upstream tags it `latest`) was a judgment call for the maintainers — the
hub is a production sync server and a 2.6 alpha changes the DocHandle
internals our sync client leans on. **Decided 2026-09-17 (Carlos): port
to 2.6.0-alpha.5 in this PR.** The experiment below is what the decision
was made on; phase 3 is the port itself.

### What changed upstream (2.5.6 → 2.6.0-alpha.5), as it affects us

- **Availability moved off the DocHandle.** The xstate machine is gone;
  `DocHandle.state` is only ever `'ready'` or `'deleted'` (and is
  `@hidden`/deprecated). Availability lives on the new `DocumentQuery`
  (`repo.findWithProgress(id)` → `peek().state` is `'loading'` /
  `'ready'` / `'unavailable'` / `'failed'`).
- **`repo.find()` never hands out an unavailable handle.** It rejects
  with `Document <id> is unavailable` once every connected peer has
  answered `doc-unavailable` (or there is no peer). `allowableStates`
  is accepted by the type but ignored. `whenReady()` on a handed-out
  handle resolves immediately. `isUnavailable()` always returns false.
- **`repo.handles` now lists a handle for every query**, including ones
  still loading or settled unavailable — and those handles report
  `state === 'ready'`. Anything that read `handles[id].state` as an
  availability signal silently reads `ready` for a missing doc.
- `DocHandle.doc()` and the change payload's `doc` are typed
  `Doc<T> | undefined` (sub-handles whose path stops resolving).
- `WebSocketClientAdapter.onError` is `() => void` and only logs;
  `disconnect()` now cancels the queued reconnect (PR 690) — the timer
  half of the bd-jit6pdwq zombie bug is fixed upstream, while a direct
  `connect()` after `disconnect()` still recreates the socket.
- A new-peer re-request of a still-wanted doc is preserved
  (`DocSynchronizer.addPeer`), which the collection-connect race fix
  depends on.

## Checklist

### Phase 1 — stable bump + range alignment

- [x] File strand bd-d08gpqvu; create worktree + topic branch
- [x] `@automerge/automerge` `^3.4.1` → `^3.5.0` in `hub-client`,
      `quarto-sync-client`, `preview-runtime`
- [x] Align `@automerge/automerge-repo*` ranges in `quarto-sync-client`
      (`^2.5.1`/`^2.5.4` → `^2.5.6`) and `preview-runtime` (`^2.5.1` →
      `^2.5.6`)
- [x] `npm install` from the repo root under Node 24; lockfile resolves
      `@automerge/automerge` 3.5.0, repo family stays 2.5.6
- [x] `deno.lock` spec lists synced by hand (the lock holds no automerge resolution entries; matches how `fb1ef319b` regenerated it)
- [x] Fast gate: ts-packages builds + vitest (schema 78, sync-client 134,
      preview-runtime 78, hub-mcp 248, sync-test-harness 8), `hub-client`
      typecheck + vitest (1217) — all green on 3.5.0 / 2.5.6
- [ ] Full gate: `cargo xtask verify` (hub-client + WASM legs included)
- [x] `hub-client/changelog.md` entry (two-commit workflow) — `4084897a9`
- [x] Commits `07b211229`, `d0b399460`, `4084897a9` pushed to
      `origin/chore/bd-d08gpqvu-automerge-npm-upgrade`; PR #685 opened
      against `main` for CI (not merged — awaiting review)

### Phase 2 — 2.6.0-alpha experiment (evidence for the decision)

- [x] On a throwaway working-tree change (never committed), bump the
      repo family to `2.6.0-alpha.5` (`next`) across all four consumers
      and run install, builds, typecheck, vitest — results below
- [x] Reverted; `node_modules` restored to the 2.5.6 resolution
- [x] Report the result and ask whether to adopt the alpha → adopt

### Phase 3 — port to 2.6.0-alpha.5

The three test failures from the experiment are the regression tests
(they were red before any code change and are the TDD "failing test"
step for this port).

- [x] Bump the repo family (`automerge-repo`, `-network-websocket`,
      `-react-hooks`, `-storage-indexeddb`, `-storage-nodefs`) to
      `2.6.0-alpha.5` in all four consumers; `npm install`
- [x] `quarto-sync-client`: `getSyncDiagnostics` / `getDocInventory`
      read the doc's verdict from the existing DocumentQuery
      (`cachedDocState`) instead of `handles[id].state`; the `change`
      listener guards the now-optional payload `doc`
- [x] `quarto-sync-client`: `StoppableWebSocketClientAdapter.onError`
      takes an optional event (upstream's handler is `() => void`);
      subclass + control-test comments updated for the upstream timer fix
- [x] `hub-client` `projectSetService.findCollectionDoc`: the second
      attempt classifies from `find()`'s rejection (unavailable →
      `not-found`, abort/timeout → `sync-unreachable`) instead of
      `allowableStates` + `handle.state`; dead `raceHandleReady` removed
- [x] `hub-client` `presenceService`: guard `doc()` returning undefined
- [x] `hub-client` debug `DocumentViewer`: state badge from
      `repo.findWithProgress(url)` subscription (`useDocHandle` can no
      longer surface `unavailable`)
- [x] `deno.lock` spec lists synced for the alpha ranges
- [x] Fast gate green on the alpha: ts-packages builds; vitest schema 78,
      sync-client 137, preview-runtime 78, hub-mcp 251, sync-test-harness 8
      (hub tier against the real Rust hub), hub-client 1217; typecheck +
      eslint clean on touched files (repo-wide eslint baseline unchanged)
- [x] Full gate: `cargo xtask verify` — all 14 steps passed (Rust: 13924
      tests, 199 skipped; hub-client build:all incl. WASM; test:ci;
      trace-viewer; preview-*; hub MCP; q2-preview-spa build)
- [x] `hub-client/changelog.md` entry (two-commit workflow) — `4084897a9`
- [x] Commits `07b211229`, `d0b399460`, `4084897a9` pushed to
      `origin/chore/bd-d08gpqvu-automerge-npm-upgrade`; PR #685 opened
      against `main` for CI (not merged — awaiting review)

#### Experiment result (2.6.0-alpha.5, 2026-09-17)

`npm install` succeeds (resolves `uuid` 14.0.2 and `bs58check` 4.0.0;
the `uuid` advisory GHSA-w5hq-g745-h8pq that `npm audit` reports against
the whole automerge-repo family on `main` is only fixable on this line).
Everything after that is red:

- **Type errors (5).** `DocHandle.doc()` now returns `Doc<T> | undefined`
  — `quarto-sync-client/src/client.ts:592` and three call sites in
  `hub-client/src/services/presenceService.ts:453-458` pass it where a
  `Doc<T>` is required. `WebSocketClientAdapter.onError` changed to
  `() => void`, so `StoppableWebSocketClientAdapter.onError(event)` no
  longer overrides it (`quarto-sync-client/src/StoppableWebSocketClientAdapter.ts:43`).
- **Behavioural change in DocHandle availability (3 test failures).**
  A document that a *live* peer lacks now resolves to state `ready`
  instead of `unavailable`:
  - `quarto-sync-client` `doc-inventory.test.ts` and
    `sync-diagnostics.test.ts`: `expected 'ready' to be 'unavailable'`.
  - `hub-client` `projectSetService.connect.test.ts` ("not-found: a live
    sync peer that lacks the document"): the failure classification
    comes back `sync-unreachable` instead of `not-found`.
  This is the exact area the sync client's cold-start "unavailable"
  retry (bd-jit6pdwq) and the project-set failure classification are
  built around, so adopting the alpha is a semantics port, not a version
  bump.
- Green on the alpha: `quarto-automerge-schema` (78), `preview-runtime`
  (78), `quarto-hub-mcp` (251), and the other 1216 hub-client tests.

Not exercised on the alpha: `npm run build:all`, e2e, the hub tier of
`sync-test-harness` against the Rust hub (samod 0.13 / automerge 0.11).

## End-to-end verification

The closest thing to a real-user path for a sync-stack bump is the
`sync-test-harness` hub tier, which spawns the real Rust hub binary
(`cargo run --bin hub`, samod 0.13 / automerge 0.11) and drives it with
the 2.6.0-alpha.5 JS client over a real websocket:

```
cd ts-packages/sync-test-harness && npm test
 ✓ src/roundtrip.test.ts (6 tests | 3 skipped) 15590ms
   ✓ hub > create project and reconnect (no delay)  3062ms
   ✓ hub > create project and reconnect (1s delay)  4026ms
   ✓ hub > create project and reconnect (5s delay)  8020ms
 Test Files  2 passed (2)
      Tests  8 passed | 3 skipped (11)
```

Output inspected: the three hub-tier cases created a project on the
Rust hub with the alpha client, disconnected, reconnected after each
delay and read the project back — i.e. the 2.6 alpha's sync protocol
interoperates with the hub's samod. Also inspected: the two
`quarto-sync-client` diagnostics tests and the hub-client
`connect.test.ts` case that were red on the bare bump now pass with
`handleState: 'unavailable'` / `kind: 'not-found'` respectively.

Not exercised locally: the Playwright e2e suites (hub-client,
q2-preview-spa) — CI's `hub-client-e2e` workflow covers those on the PR.
