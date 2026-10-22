---
title: 'Duplicate-seq self-heal for file documents (bd-0h8crv6u)'
date: 2026-10-22
description: 'Extends the index-document duplicate-seq self-heal to file documents, adding per-document backoff and the unavailable-files retry poll as a circuit breaker.'
status: implemented
braid:
  strand: bd-0h8crv6u
  priority: P1
---

## Overview

When a duplicate-seq actor collision hits a *file* document, the receiving
hub-client never recovers: automerge-repo logs `[automerge-repo:repo] error
handling inbound message RangeError: duplicate seq N found for actor <id>`
and drops the message, and because the throw happens before sync state
advances, the hub re-sends the same colliding message forever. That file's
sync is wedged for that browser until the user manually clears the
automerge IndexedDB database.

The index document has self-healed since bd-6f21d4c6
(`installDuplicateSeqRecovery` / `recoverIndexDocument` in
`ts-packages/quarto-sync-client/src/client.ts`, plan
`2026-09-17-index-doc-duplicate-seq-self-heal.md`); file documents were
deliberately scoped out as "materially more code and test surface". A
production console log observed on 2026-10-22 (colliding actor
`b4fa8892…`, 64 hex chars — the legacy stable-actor format, not a random
post-transition actor) showed the RangeError with no recovery breadcrumb,
consistent with a file-document collision on a current build, or any
collision on a pre-self-heal build.

This plan generalizes the self-heal to file documents, with two safety
additions the index-only version did not need:

1. **Per-document backoff.** A legacy client *actively editing* under a
   shared stable actor re-wedges the document after every recovery, so
   without a cap each colliding message triggers a full discard + refetch.
   Refetches are not cheap in the worst case: binary file contents are
   stored inline in their automerge document, and no automerge compaction
   exists anywhere in the system (verified by grep across
   `ts-packages/quarto-sync-client` and `crates/quarto-hub`), so document
   size — and refetch cost — grows monotonically over a project's
   lifetime.
2. **A circuit breaker.** After the attempt cap, the file is routed into
   the existing `state.unavailableFiles` bounded retry poll — the same
   machinery that already handles dangling index entries — instead of an
   unbounded recovery loop.

Root cause and prevention are unchanged and out of scope here:
pre-transition clients mint stable per-user actor ids (plan
`2026-09-30-automerge-author-id-transition.md`); the version-skew window
closes with bd-8oyidg7h (removing `GET /auth/actor`).

## Checklist

### Phase 1 — Tests first (TDD)

- [x] New `full-stack-file-collision.test.ts` in
  `ts-packages/quarto-sync-client/src`. The index version's template,
  `full-stack-actor-collision.test.ts`, no longer exists: it was deleted
  in `5ec72ca61` (author-ID transition phase 2) because forcing two
  `SyncClient`s to share an actor through `connect`/`createNewProject`
  became inexpressible once those APIs stopped taking actor ids. The new
  test therefore forces the collision with a raw `Repo` +
  `NodeWebSocketClientAdapter` playing the legacy peer: `repo.import()` a
  divergent copy of the file document doctored at the bare-automerge level
  (`clone(doc, { actor })`, the recipe in `actor-id-collision.test.ts`),
  synced through a real in-process hub (`startTestHub()`) to the victim
  `SyncClient`, with each side committing its edit synchronously so
  neither processes the other's message first. **Ordering mechanism
  (added at implementation):** the test-hub WS relay is hub-and-spoke —
  clients sync only with the hub's repo, never directly with each
  other — so exactly the client whose local divergent edit *loses the
  race to the hub* wedges permanently (the hub holds and re-offers the
  winning change, which collides with the loser's local copy forever).
  To make that loser deterministic, the victim connects through a
  traffic-gating `NetworkAdapter` wrapper (the existing
  `setNetworkAdapterWrapper` seam) that queues inbound+outbound
  messages: victim edits while gated (its change stays local), the
  legacy peer's doctored change lands on the hub first, then the gate
  opens and both collide. Assert:
  1. The collision + swallowed catch really happen (matching `RangeError`
     logged via whichever shape the installed automerge-repo uses).
  2. The recovery warning fires for the file document.
  3. A further edit to that file from an uninvolved third client — which
     without the fix never reaches at least one colliding client — reaches
     both, within a bounded window.
  4. A second later edit also reaches both (recovery is durable).
- [x] Red-then-green gate: run the new test against unmodified code and
  confirm it fails the expected way (collision occurs, no recovery log,
  run consumes the full poll timeouts). Only then implement.
- [x] Backoff unit test: a peer that keeps sending colliding
  messages triggers at most `RECOVERY_MAX_ATTEMPTS` recoveries per
  document, spaced by at least `RECOVERY_COOLDOWN_MS` — assert via a
  `repo.find` call-count spy (findDoc's inner call). **Deviation from
  the draft:** real timers + an injected tiny `recoveryBackoff` config
  (new `ConnectOptions` field, mirroring the `findDocRetry` precedent)
  instead of fake timers — fake timers don't compose with the real
  websocket I/O the recovery's refetch needs, and findDoc's own
  `AbortSignal.timeout` would fire on any timer advance. Collisions are
  driven deterministically by stubbing the victim's per-document
  `DocSynchronizer.receiveMessage` to throw the duplicate-seq RangeError
  and invoking the (wrapped) `repo.synchronizer.receiveMessage` directly
  (`client.getRepo()` is the seam).
- [x] Circuit-breaker test: after the cap, the file is marked unavailable
  (`markFileUnavailable` + `onFileUnavailable` callback +
  `startUnavailableRetry` kicked). Then, with the divergent peer silent,
  the retry poll re-fetches and the file flows back through `onFileAdded`
  (this healing is only possible because the capped recovery deleted the
  local copy first — see the circuit-breaker item in Phase 2).
- [x] Regression tests: **(corrected at implementation — the "existing
  full-stack index-collision test" no longer exists; it was deleted in
  `5ec72ca61` as the first checklist item notes, so there is nothing to
  keep green. The index recovery path has had no positive coverage since.)
  Add a stub-driven index-routing regression test** (same DocSynchronizer
  stub technique, index doc id) asserting `recoverIndexDocument` still
  fires for the index document after the routing generalization; plus a
  collision whose `message.documentId` matches no tracked document (not
  the index, not a known file) triggers no recovery attempt.
- [x] `actor-id-collision.test.ts` (the bare-automerge repro) needs no
  file-document-shaped variant: its repro doc (`{ text: string }`) already
  is file-doc-shaped and the mechanism is doc-shape-agnostic. Resolved at
  plan review; nothing to add.

### Phase 2 — Implementation

All in `ts-packages/quarto-sync-client/src/client.ts` unless noted.

- [x] Generalize `handleCollision` routing by `message.documentId`: the
  connection's index document goes to the existing `recoverIndexDocument`;
  a tracked file document goes to the new `recoverFileDocument`; anything
  else is ignored (today's behavior for non-index documents). Resolve the
  document id to a path by scanning `state.fileHandles` for
  `handle.documentId === message.documentId` — `recoverIndexDocument`
  already relies on this exact comparison, the scan runs only on the error
  path, and it needs no new state. (A maintained reverse map was
  considered at plan review and rejected: it would have to be kept in
  sync across `subscribeToFile` / `subscribeToFileInternal`, the
  removed-files branch of `syncWithFiles`, `renameFile`, `deleteFile`,
  and `disconnect()`.)
- [x] Track change handlers in a `Map<DocumentId, handler>` — keyed by
  document id, not path, so `renameFile` needs no re-key and recovery
  looks the handler up directly from `message.documentId` — so a recovery
  can detach the old handle's subscription without waiting for
  `disconnect()` (today the unsubscribe closures are anonymous entries in
  `state.cleanupFns`, unreachable for a targeted swap). Populate in
  `subscribeToFile` / `subscribeToFileInternal`; remove wherever
  `state.fileHandles` entries are dropped (the removed-files branch of
  `syncWithFiles`, `deleteFile`, `disconnect()`).
- [x] `recoverFileDocument(documentId)`, mirroring `recoverIndexDocument`:
  per-document in-flight guard; backoff check (below); `repo.delete()`;
  `findDoc<FileDocument>()` re-fetch; the `state.repo` race guard against
  a racing `disconnect()`/`connect()`; detach old handler; resubscribe via
  `subscribeToFile`.
- [x] Notification semantics: re-fire `onFileAdded` with the fresh content
  (no new callback). Consumers fully replace state on `onFileAdded` —
  `vfsAddFile(path, text)` in
  `ts-packages/preview-runtime/src/automergeSync.ts` — so this rebuilds
  the VFS, preview, and editor model from the recovered document. Record
  the Monaco cursor/undo-history disruption on a recovery of the open file
  as accepted UX (the alternative is a permanently stale file).
- [x] Backoff state: `Map<DocumentId, { attempts, lastAt }>` with
  `RECOVERY_COOLDOWN_MS` (start: 30_000) and `RECOVERY_MAX_ATTEMPTS`
  (start: 3). Reset policy (decided at plan review): quiet window — reset
  `attempts` when `now - lastAt` exceeds a threshold (start: 10 ×
  cooldown), evaluated lazily inside the recovery path. The alternative,
  resetting when an inbound sync message for the document is handled
  without throwing, was rejected: it puts a branch on the
  every-inbound-message hot path and couples the generic message wrapper
  to recovery bookkeeping for no real gain. Document the choice in the
  code comment.
- [x] Circuit breaker: at the cap, `repo.delete(documentId)` *first*, then
  `markFileUnavailable(path, docId)` and `startUnavailableRetry()`, and
  stop scheduling recoveries for the document while it is marked
  unavailable. The delete is load-bearing, not hygiene: the retry poll's
  plain `findDoc` would return the same wedged handle (the poison lives in
  the local DocSynchronizer's per-peer sync state), so without the delete
  the poll can never heal a circuit-broken file. Confirm `repo.delete()`
  is a no-op for a document the repo no longer holds (repeat collisions
  while the file is marked).
- [x] Console breadcrumbs mirroring the index wording
  (`[quarto-sync-client] file document <id> (<path>) hit a duplicate-seq
  actor collision …` / `recovered file document …` / cap-reached warning),
  so production logs distinguish index from file recoveries.

### Phase 3 — Verification and rollout

- [x] Consumer inventory: every holder of file content or file-doc state —
  the Monaco editor path (`hub-client/src/hooks/useAutomergeSync.ts`), the
  preview VFS (`ts-packages/preview-runtime/src/automergeSync.ts`), binary
  handling, link diagnostics (Q-13-4) — confirmed to rebuild correctly from
  a re-fired `onFileAdded`. Fix any consumer that caches derived state the
  callback does not refresh.
- [x] Full `quarto-sync-client` package suite green; `npx tsc --noEmit`
  clean in both `quarto-sync-client` and `hub-client` (which bundles it
  from source).
- [x] `npm run build:all` in `hub-client/` (production build is stricter
  than tests alone).
- [x] E2E: verification tops out at the in-process full-stack test
  (decided at plan review). A two-context Playwright repro is impractical
  for the same reason `full-stack-actor-collision.test.ts` was deleted in
  `5ec72ca61`: a shared-actor collision cannot be forced through the
  current `SyncClient` API, and doctoring a raw-repo legacy peer is only
  expressible in-process. The hub-client e2e-author spec's existing
  duplicate-seq-absence assertion stays as-is. State explicitly in the
  completion report that verification stopped at the in-process
  full-stack test, per the end-to-end verification rule.
- [x] Changelog: if any `hub-client/` file changes (e2e spec, wiring),
  follow the two-commit `hub-client/changelog.md` workflow. A
  sync-client-only change does not require it.

## Details

### Why the index-only scope no longer holds

The original scope-out was a deliberate first-pass decision
(`2026-09-17-index-doc-duplicate-seq-self-heal.md`, "Out of scope"), made
before the author-ID transition shipped. What changed:

- The transition (2026-09-30) removed stable actor minting from *current*
  clients, but legacy clients still emit them during the skew window —
  and a cached SPA tab can stay open for weeks, so the window is long.
- The 2026-10-22 production log shows the collision surfacing in the wild
  on a file the self-heal does not cover.
- The manual remedy (clear IndexedDB) is undiscoverable for users and
  discards *all* local state, not just the wedged file.

### Performance shape (from the 2026-10-22 analysis)

| Factor | Cost |
| --- | --- |
| Detection (steady state) | Zero — the wrapper already intercepts every inbound message; routing runs only on the error path |
| Single recovery, text file | One `repo.delete` + full refetch — same order as the initial connect load |
| Single recovery, binary file | Refetch pulls the inline bytes over the websocket |
| Long-lived document | Refetch cost grows with un-compacted history |
| Legacy peer actively editing | Re-wedges after every recovery → the reason backoff is mandatory, not optional |

### Design decisions to record during implementation

1. **Whether to factor a shared delete + refetch core** out of
   `recoverIndexDocument` and `recoverFileDocument`, or keep two
   near-duplicate functions while the file variant settles (the index
   version reconciles identity/capture state the file version does not
   need; premature sharing was a stated concern in the original plan's
   factoring of `reconcileIndexDoc`).
2. **Cap and cooldown values** — 3 attempts / 30 s are starting points;
   validate against the storm scenario in the backoff test.

### Decisions recorded during implementation

- **Factoring (decision 1):** two near-duplicate functions were kept.
  `recoverIndexDocument` reconciles identity/capture/folder state and
  re-runs `migrateIndexDocument`; `recoverFileDocument` carries the
  backoff + circuit breaker the index path does not need. The shared
  core would be ~15 lines (guards, delete, refetch, repo-race check)
  with two divergent tails — sharing would couple two settling
  surfaces for little gain, the concern the original plan raised over
  `reconcileIndexDoc`.
- **Cap and cooldown values (decision 2):** defaults stay 3 attempts /
  30 s cooldown / 300 s quiet window. The storm scenario is validated
  by the backoff test with millisecond configs injected via
  `ConnectOptions.recoveryBackoff`.
- `repo.delete()` is confirmed a no-op for a document the repo no
  longer holds (Repo.delete guards on the query's presence before
  touching handles/storage; it always clears sync-state tracker and
  storage entries that exist), so repeat collisions while a file is
  circuit-broken are safe.
- Backoff config is injectable per connection
  (`ConnectOptions.recoveryBackoff` / `CreateProjectOptions.recoveryBackoff`,
  resolved like `findDocRetry`) so tests can use millisecond cooldowns
  with real timers; production defaults stay 3 attempts / 30 s cooldown
  / 300 s quiet window.
- The test-hub relay being hub-and-spoke (verified in
  `WebSocketServerAdapter`: `send` targets one socket by `targetId`;
  clients never see each other's sync messages) also means a JS test
  hub can never *hold* two divergent changes — it accepts the first and
  wedges on the second. The permanently-wedged victim is therefore
  always the race loser, matching the production observation ("wedged
  for that browser", singular).

### Out of scope

- Preventing the collision (bd-8oyidg7h removes `GET /auth/actor`;
  upstream automerge-repo recovery remains absent).
- Hub-side (samod) handling of colliding inbound changes from a legacy
  client — worth a separate look: if the hub itself wedges or relays the
  divergent history to other clients, the blast radius is larger than one
  browser. File as a follow-up strand if investigation confirms it.
- Automerge history compaction (would bound refetch cost globally; no
  compaction exists anywhere in the system today).

## Completion report (2026-10-22)

Verification topped out at the in-process full-stack test, per the plan
(a two-context Playwright repro is inexpressible for a forced
shared-actor collision through the current `SyncClient` API — the same
reason `full-stack-actor-collision.test.ts` was deleted in `5ec72ca61`).
**Per the end-to-end verification rule: this feature was not exercised
through a real browser session; verification stopped at the in-process
full-stack test below.**

- Red gate: `npx vitest run src/full-stack-file-collision.test.ts` on
  unmodified code failed exactly as specified — the collision and the
  swallowed automerge-repo catch were observed, the recovery warning
  never fired, the run consumed the full poll window
  (`expect(sawRecoveryStart).toBe(true)` → false). The backoff and
  circuit-breaker tests likewise timed out waiting for a first recovery.
- Green: all 5 tests in `full-stack-file-collision.test.ts` +
  `file-collision-recovery.test.ts` pass; full `quarto-sync-client`
  suite 191/191; hub-client suite 462/462; `npx tsc --noEmit` clean in
  both packages; `npm run build:all` in hub-client green;
  `cargo xtask lint` clean.
- The full-stack test's output was inspected, not inferred: with the
  fix, the run shows `[quarto-sync-client] file document <id>
  (main.qmd) hit a duplicate-seq actor collision (bd-0h8crv6u);
  discarding the local copy and re-fetching from the hub` followed by
  `recovered file document <id> (main.qmd)`, the victim converging on
  the hub's content (`edit from the legacy peer\n` — its own unwinnable
  edit discarded, as designed), and both post-collision edits from the
  third client reaching victim and witness.
- Consumer inventory: every holder of file content rebuilds from a
  re-fired `onFileAdded` — preview-runtime's `vfsAddFile` /
  `vfsAddBinaryFile`, App.tsx's `fileContents` map (Monaco picks it up
  via diff-sync; cursor/undo disruption on the open file is the plan's
  accepted UX), the search provider's `addOrUpdate`, binary version
  counters, and the sync client's own AST cache via
  `tryParseAndNotify`. Q-13-4 link diagnostics are computed by the
  render pipeline from the rebuilt VFS, so they refresh on the next
  render. No consumer caches derived state the callback does not
  refresh.
- No `hub-client/` file changed (e2e spec untouched, no wiring), so the
  two-commit `hub-client/changelog.md` workflow is not required.
