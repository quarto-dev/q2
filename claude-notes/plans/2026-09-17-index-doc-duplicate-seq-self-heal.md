# Automatic self-heal for a duplicate-seq-stuck index document

**Strand:** bd-6f21d4c6. **Retroactive plan** — written after the design was
implemented and verified, to document it for review rather than to drive the
work (see CLAUDE.md's "Where information lives" — this replaces a larger,
investigation-shaped research doc that also covered mechanisms this PR does
not fix).

## Overview

A hub-client user's local `IndexDocument` replica can permanently stop
receiving peer updates — surfacing as Q-13-4 Monaco squiggles on links to
files that genuinely exist — with no recovery short of manually clearing the
browser's `automerge` IndexedDB database.

Root cause: `actorIdFromUserId` (`hub-client/src/services/userSettings.ts`)
deliberately derives a *stable* Automerge actor id from the user's local
`userId`, reused across sessions/tabs. If two sessions for the same user each
commit a local edit to the same document before either has seen the other's,
both changes can claim the same `(actor, seq)` pair. When the sync protocol
later tries to exchange them, `automerge`'s `receiveSyncMessage` correctly
throws `RangeError: duplicate seq N found for actor <id>` (this is not a bug —
two changes really do claim the same identity) — but `automerge-repo`'s own
`Repo#receiveMessage` swallows that throw with a bare
`console.log("error receiving message", ...)` and never recovers. Because the
throw happens before either side's sync state advances, the hub keeps
re-sending the same colliding message on every subsequent attempt, and the
affected client keeps rejecting it, forever. This is confirmed directly
against the vendored `automerge`/`automerge-repo` v2.5.6 source and
reproduced end-to-end (`full-stack-actor-collision.test.ts`): two real
`SyncClient`s sharing one actor id, racing a local edit each, hit the exact
swallowed catch through a real hub relay.

This plan covers only **getting an already-stuck client unstuck**, not
preventing the collision itself (that needs a change to `actorIdFromUserId`
or an upstream `automerge-repo` fix — out of scope here, tracked as future
work on bd-6f21d4c6).

## Checklist

- [x] Reproduce the swallowed-catch mechanism at the bare-automerge level
      (`actor-id-collision.test.ts`)
- [x] Reproduce it end-to-end through the real `SyncClient`/`Repo`/hub-relay
      stack, including the resulting permanent stall
      (`full-stack-actor-collision.test.ts`)
- [x] Decide the fix's detection mechanism and recovery scope (see Design
      decisions below)
- [x] Implement `installDuplicateSeqRecovery` / `recoverIndexDocument` in
      `ts-packages/quarto-sync-client/src/client.ts`
- [x] Verify red-then-green against the same repro: temporarily disable the
      fix, confirm the repro's assertions fail; restore, confirm they pass
- [x] Confirm no regressions: full package suite green, `tsc --noEmit` clean
      in both `quarto-sync-client` and `hub-client` (which bundles it from
      source)

## Design decisions

Two decisions were made explicitly (options weighed, one chosen) before
implementation:

**Detection mechanism — wrap `repo.synchronizer.receiveMessage` (chosen)
vs. sniff `console.log` (rejected for now).** `Repo.synchronizer` is a public
field (`CollectionSynchronizer`); `receiveMessage` is a public, documented
method. Wrapping it at the *instance* level (not the class prototype) gives
structured access to `message.documentId` — no string parsing — and calls
through to the original method, tapping its rejection with an additional
`.catch()` rather than replacing it, so `Repo.ts`'s own existing swallow
still fires unchanged. This can't affect any other `Repo` instance, and no
`automerge-repo` source is edited. The rejected alternative (wrapping global
`console.log` for the literal `'error receiving message'` string) would
match the error's own wording exactly and need no reach into repo internals,
but is a global side effect requiring careful wrap/restore discipline, and
depends on exact log text staying stable across library versions.

**Recovery scope — index document only (chosen) vs. generalize to any
tracked file document (rejected for now).** Matches the actually-reported
symptom exactly and keeps the change small: one handle to swap
(`state.indexHandle`), one resubscribe path to re-run. Generalizing to file
documents would cover a broader class of "this document is stuck" bugs but
is materially more code and test surface for a first pass.

## Implementation

`ts-packages/quarto-sync-client/src/client.ts`:

- `installDuplicateSeqRecovery(repo, indexDocId)` — called from both
  `connect()` and `createNewProject()` right after `state.repo = new
  Repo(...)`; its uninstall function is pushed onto `state.cleanupFns`
  (restored on `disconnect()`). Wraps `repo.synchronizer.receiveMessage`;
  on a rejection matching `message.documentId === indexDocId` and
  `err instanceof RangeError` with `/duplicate seq \d+ found for actor/`,
  calls `recoverIndexDocument(message.documentId)`. Defensive: if
  `repo.synchronizer` isn't the expected shape (some tests mock `Repo`
  without one), it's a no-op rather than a crash.
- `recoverIndexDocument(collidedDocumentId)` — guarded against concurrent
  re-entry (`indexRecoveryInFlight`). Does the scoped, automated analog of
  the manual IndexedDB wipe, entirely through `Repo`'s own public API:
  - `repo.delete(collidedDocumentId)` — confirmed against source
    (`Repo.ts`'s `delete()` + its `"delete-document"` listener) to call
    `synchronizer.removeDocument()` (drops the `DocSynchronizer` and all
    its per-peer `SyncState` for that one document — the wedged
    bookkeeping) and `storageSubsystem.removeDoc()` (purges the persisted
    bytes for that one document only).
  - `findDoc<IndexDocument>(collidedDocumentId)` (the same retry-aware
    helper `connect()` already uses) re-fetches a brand-new `DocHandle`
    with a fresh `SyncState`, forcing a clean full resync from the hub.
  - Detaches the old (now-`DELETED`) handle's `'change'` subscription,
    swaps in the fresh handle as `state.indexHandle`, re-attaches the
    subscription, and reconciles file/identity/capture state against the
    fresh doc's current content (a `'change'` event only fires on *future*
    mutations, so a one-time catch-up call is needed).
  - A race guard mirrors an existing pattern in `findDoc`'s own retry loop:
    if `state.repo` no longer matches the `Repo` this recovery started
    against (a `disconnect()`/new `connect()` raced it), the stale result
    is discarded rather than applied to unrelated new connection state.
- `reconcileIndexDoc`/`attachIndexSubscription` — shared helpers factored
  out of the (previously duplicated, one-copy-each-in-`connect()`-and-
  `createNewProject()`) inline `'change'` handler, so the normal
  live-subscription path and the post-recovery catch-up path can't drift
  apart.

## Verification

`full-stack-actor-collision.test.ts` drives two real `SyncClient`s sharing
one actor id against a real in-process hub (`startTestHub()`), racing a
local edit from each (committed synchronously, no `await` in between, so
neither client's `Repo` can process the other's message first). It asserts:

1. The collision + swallowed catch really happen — a matching
   `RangeError` logged via whichever shape the installed automerge-repo
   uses (see "Automerge-repo version compatibility" below).
2. The fix's recovery log fires for this connection's index document.
3. A further, unrelated edit from an uninvolved third client — which
   previously would never reach at least one of the two colliding
   clients — now reaches **both**, within a bounded window.
4. A second later edit also reaches both, confirming the recovery is
   durable, not a one-off catch-up.

Per the repo's TDD rule, this was verified red-then-green: the two
`installDuplicateSeqRecovery` call sites were temporarily commented out,
the test was confirmed to fail exactly as expected (collision still
occurs; no recovery log; the run consumes the full poll timeouts), then
the fix was restored and the test confirmed green — consistently, across
repeated runs (~0.4–0.6s each, vs. ~10.3s before the fix, spent waiting on
a stall that never resolved).

`actor-id-collision.test.ts` separately proves, at the bare-automerge
level (no `automerge-repo`, no network, no hub), that reusing one actor id
across two independently-edited copies of a document reliably produces
this exact `RangeError` via the real sync-message exchange — the fact
`installDuplicateSeqRecovery`'s detection regex depends on.

Full package suite: 140/140 (one existing-test compatibility fix needed:
three existing test files mock `Repo` without a `.synchronizer`, which
crashed `connect()` until the defensive guard above was added).
`npx tsc --noEmit` clean in both `quarto-sync-client` and `hub-client`.

### Automerge-repo version compatibility

Caught by CI, not locally: this branch's merge-base predates
`d0b399460` ("chore(npm): port automerge-repo family to 2.6.0-alpha.5",
bd-d08gpqvu), which landed on `main` while this fix was in flight and
changed `CollectionSynchronizer#receiveMessage`'s shape — async
(returning a rejecting `Promise<void>`, swallowed by `Repo`'s own
`.catch`) in v2.5.6, the version this fix was first written and tested
against; **synchronous** (`void`, a plain throw caught by a `try/catch`
around `Repo`'s inbound-message dispatch) from v2.6.0-alpha.5, which
also moved the log line from `console.log("error receiving message",
{ err, message })` to `console.error("[automerge-repo:repo]", "error
handling inbound message", err)` (no `message` in the log args anymore).

The underlying mechanism — automerge's `receiveSyncMessage` still throws
the identical `RangeError` synchronously either way; automerge-repo
still only logs and drops it, with no recovery — is unchanged, so the
fix's *design* didn't need to change, only its *code*:
`installDuplicateSeqRecovery`'s wrapper now handles both shapes (checks
whether `original(message)` returned a `Promise` before deciding whether
to tap `.catch` or rely on a synchronous `try/catch`), so a *future*
automerge-repo version bump doesn't silently disable it again. Re-ran
the full red-then-green verification against v2.6.0-alpha.5 after
adapting: same result, collision still occurs, no recovery without the
fix, recovery restored with it.

## Out of scope (deliberately)

- **Preventing the collision.** Needs either a q2-side change to
  `actorIdFromUserId` (combine the stable, attribution-carrying prefix with
  a per-session-unique suffix) or an upstream fix (`automerge-repo`'s own
  abandoned `stable-actor-ids` branch, and/or making `Repo.ts`'s catch
  actually recover instead of only logging). Neither is attempted here.
- **File documents.** Only the index document self-heals; a file document
  hitting the same collision is not covered.
- **Confirming this against a real production incident.** This fixes a
  manufactured-but-real repro of the mechanism; it has not been observed
  fixing an actual user's stuck session.
