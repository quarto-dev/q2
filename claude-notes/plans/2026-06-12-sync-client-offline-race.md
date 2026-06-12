# Browser sync offline-fallback race family

**Strand:** bd-10bdjmjb (related: bd-8x482xb0 dangling index entry,
bd-vm5e5u10 MCP hard-fail on dangling entries; discovered-from
bd-p68lx71t, the 2026-06-12 incident)
**Status:** DESIGN — for Carlos's review; no implementation until
go-ahead.

## Overview

During the 2026-06-12 quarto-hub.com incident, colleagues saw
"failures to open automerge documents." The deployed changes were
reverted, but the failures reproduced on the rolled-back build and in
private windows, and Carlos's console trace pinned the real mechanism
— which is long-standing, not shipped that day:

```
Waiting for peer connection...
Peer connection failed, continuing in offline mode: Error: Timeout waiting for peer connection
...
Peer connected - switching to online mode        ← arrives moments later
```

The document path in hub-client is `App.tsx` →
`@quarto/preview-runtime` `automergeSync.connect(…)` →
`quarto-sync-client` `connect(…, peerTimeoutMs ?? 1)`
(`ts-packages/preview-runtime/src/automergeSync.ts:159`,
`ts-packages/quarto-sync-client/src/client.ts:495`). A **1 ms** peer
wait always loses against real network latency, so every session
starts in "offline mode" against IndexedDB and depends entirely on
what happens after the late "switching to online mode" transition.

This is the **browser twin of bd-xnmd5ni1** — the same library default
whose silent offline fallback made the MCP server fabricate project
creation (fixed 2026-06-12 with `requireOnline`). The browser can't
use `requireOnline` (offline-first against IndexedDB is a *feature*
there); it needs the fallback to actually be safe instead.

## The three defects

### D1 — documents created during the offline window never reach the hub

Evidence: `/cscheid/hello-claude.qmd` on the playground. The index
entry (a change to the already-synced index document) propagated to
the hub; the newly created file document did not — verified absent
from the hub's storage shard (`/mnt/hub-data/automerge/3H/`, no
`JoqsM…`), and **still absent after the creating profile reconnected
online and hard-refreshed**. So reconnection does not (re)announce
locally created documents. Every fetch of that document by any other
client fails forever ("Document … is unavailable"), and the only copy
lives in the creator's IndexedDB. This is the data-loss-shaped defect
and the priority.

Open mechanism question the red test must answer: automerge-repo's
collection synchronizer is *supposed* to add all repo documents to a
newly connected peer, so the gap is one of:
- (a) client-side: the SPA's repo/adapter lifecycle (fresh adapter or
  repo per reconnect; bd-jit6pdwq's Stoppable adapter; the doc living
  in a different `Repo` than the reconnecting one) drops
  created-while-offline docs from what gets announced;
- (b) hub-side: samod's share/accept policy ignores announcements of
  documents it has never seen (vs. documents it has and can serve);
- (c) something else the repro will surface.
Phase 1 exists precisely to localize this before designing the fix —
the fix differs materially between (a) and (b).

### D2 — opens that failed during fallback never retry once online

`connect()` runs `loadFileDocuments(files)` exactly once; the
`onPeerConnect` handler (`client.ts:589-595`) only flips
`currentlyOnline` and fires `onConnectionChange`. A cold-IndexedDB
session (private window, first visit, new machine) therefore fails
its opens 50–500 ms before the connection lands, and nothing reloads
them — the user sees "Document unavailable" until a full reload. The
existing `findDocRetry` machinery (cold-start "unavailable" retry,
bd-jit6pdwq) softens some paths but demonstrably not this one.

### D3 — the 1 ms default is a mis-applied optimization

The 1 ms wait exists so warm-IndexedDB reloads render instantly
(hub-client's offline-first design goal — legitimate and worth
preserving). But as the *network* policy of a server-backed
collaborative app it guarantees the fallback path runs on every single
session, making D1/D2 everyone's steady state rather than a rare
degraded mode. q2-preview already solved this shape for its SPA with
health-arbitrated boot (bd-jit6pdwq Phase 2, e798a9df): HTTP `/health`
decides "server reachable?" — websocket peer-wait then gets a generous
budget, IndexedDB remains the fast path for rendering, but
*server-truth operations* don't pretend to be offline.

## Fix design

Ordered so each part lands independently and D1 (data loss) goes
first. All in q2; no hub deployment required until rollout.

### Part 1 — make created documents survive (D1)

1. **Red test first, to localize**: in-process automerge-repo "hub"
   (the `test-hub.ts` harness from the MCP work) + a sync-client
   session whose adapter connects *late* (deterministic: hold the
   upgrade until after `createNewProject`/`createFile` completes in
   fallback mode). Assert the hub's repo eventually holds the created
   document. This fails today and tells us *where* the announcement
   dies (client vs hub) by inspecting which protocol messages cross
   the wire.
2. **Fix accordingly**:
   - If (a) client-side: track documents created while
     `!currentlyOnline` in the sync-client; on the `peer` event,
     explicitly (re)announce/flush them (automerge-repo exposes the
     handles; worst case, a no-op `change` or explicit
     `repo.networkSubsystem`-level announce). Covers both
     `createNewProject` and per-file creation paths.
   - If (b) hub-side: adjust the samod share/accept policy in
     `crates/quarto-hub` so announced-unknown documents are accepted
     and persisted (with the existing audit/access policy applied) —
     plus the same client-side test rides along as the regression
     guard.
3. **Storage health check first, recovery later** (Carlos,
   2026-06-12: no mutations to automerge documents until a
   report-only instrument exists). A `hub doctor` subcommand
   (working name) that is **read-only by construction** in its first
   incarnation — there is no write path to gate behind a flag because
   none is implemented:
   - Scans a storage directory and reports, human-readable and
     `--json`:
     a. *dangling index entries* — index `files` entries whose
        document is absent from storage (the hello-claude class);
     b. *unloadable documents* — present but failing to load as
        automerge (corruption);
     c. *orphan documents* — in storage, referenced by no index
        (informational);
     d. summary counts + nonzero exit when (a)/(b) found, so it can
        run under cron as a standing health check.
   - Index documents are identified by attempt-parsing every doc
     against the `IndexDocument` schema (the hub does not keep a
     project registry; all docs are stored uniformly).
   - Runs **offline against a copy** of the data dir (the live store
     is lock-guarded; `/mnt/hub-data` is ~20 MB, and DLM snapshots
     exist) — zero interaction with the running server. A live admin
     endpoint is a possible later convenience, not v1.
   - Immediate use once built: run against quarto-hub.com's storage
     to size the damage — are there dangling entries beyond
     `hello-claude.qmd`, and since when? That number is incident
     data for bd-p68lx71t.
   - **Mutation (cleanup/repair) is a separate, later work item**
     with its own go-ahead: remove-dangling-entry and
     restore-from-client flows, designed only after the report has
     told us what production actually looks like. Restoration of
     held-in-browser bytes becomes possible automatically once the
     D1 fix ships and the creator reopens the project.

### Part 2 — self-heal failed opens on the online transition (D2)

In `connect()`'s `onPeerConnect` (and the matching handler in the
`createNewProject` path), when transitioning offline→online:
re-run `loadFileDocuments` for documents that previously failed
(track failures during the initial pass), firing the normal
`onFileAdded`/`onFileChanged` callbacks so the UI recovers without a
reload. Idempotent (already-loaded docs skipped), bounded (one retry
sweep per transition). Red test: cold-storage connect against a
late-connecting hub; assert the files surface after the peer event
without calling `connect()` again.

### Part 3 — honest connection policy for hub-client (D3)

Replace the bare 1 ms default at hub-client's call site (via
`preview-runtime`'s `automergeSync.connect`) with the
health-arbitrated pattern q2-preview already uses: probe `/health`
(HTTP, immune to websocket handshake stalls); if the server is
reachable, wait for the peer with a realistic budget (seconds) before
declaring offline — while still letting IndexedDB render content
immediately (rendering fast-path and connection policy decoupled).
If `/health` is unreachable, offline mode is *true* and the existing
behavior is correct. The sync-client already accepts the options bag
(`peerTimeoutMs`, bd-jit6pdwq), so this is mostly a `preview-runtime`
/ hub-client change plus defaults documentation. The 1 ms library
default itself stays (changing it silently would re-run bd-xnmd5ni1's
lesson in reverse for other callers); each caller states its policy.

### Explicitly out of scope here

- bd-vm5e5u10 (MCP `connect_project` should degrade gracefully on a
  dangling entry rather than brick the project) — separate small fix,
  same test harness, can ride along in the same review if desired.
- Re-rollout of the `--additional-audiences` deployment change
  (blocked on this work only operationally — one commit + workflow +
  deploy when we're ready; see deployment repo bd-erf hazards note:
  S3 still holds the stale allowlist and `latest/` still points at
  the 2026-06-12 build, so the workflow must re-run before any deploy).
- The bd-jit6pdwq bisection (bd-p68lx71t): if colleagues confirm
  recovery post-rollback, the reframed diagnosis (longstanding race,
  restart-amplified) likely closes it as "not a new regression," with
  this plan as the fix vehicle.

## Test plan summary (TDD)

| Test | Asserts | Fails today |
| --- | --- | --- |
| created-while-offline doc reaches hub after peer connect | D1 | yes |
| index entry + doc arrive atomically enough that a second client can open the file | D1 | yes |
| cold-storage opens self-heal on online transition without re-connect | D2 | yes |
| warm-IndexedDB reload still renders without waiting for the peer | D3 regression guard | no (must stay green) |
| health-reachable ⇒ no "offline mode" console path in hub-client boot | D3 | yes (behavioral) |

Harness: the in-process automerge-repo hub from the MCP e2e work
(`test-hub.ts`, gains a "hold upgrades until released" switch — the
`acceptWs` knob generalized), plus the real `crates/quarto-hub` binary
for one integration pass if Phase 1 localizes to the hub side.

## Phases / work items

### Phase 1 — localize D1 (red tests)
- [ ] test-hub harness: deferred-upgrade switch (connect succeeds only
      after the test releases it)
- [ ] Red test: created-while-offline document must reach the hub
- [ ] Wire-level localization: client never announces vs hub ignores
      announce (record verdict here — it gates the Part 1 design)

### Phase 1.5 — `hub doctor` (report-only storage health check)
- [ ] Tests: fixture storage dirs (healthy / dangling entry /
      corrupt doc / orphan) → exact report + exit codes; `--json`
      schema locked by test
- [ ] `hub doctor <data-dir>` subcommand, read-only by construction
- [ ] Run against a copy of quarto-hub.com's storage; record findings
      here and on bd-p68lx71t (damage sizing)

### Phase 2 — fix D1
- [ ] Fix per Phase 1 verdict (client re-announce or hub accept-policy)
- [ ] Second-client open test goes green
- [ ] (gated on its own go-ahead, after doctor findings) repair mode
      design: remove-dangling-entry / restore-from-client; playground
      cleanup or recovery of `hello-claude.qmd`

### Phase 3 — D2 self-healing opens
- [ ] Red test + fix: failed loads retried on online transition,
      callbacks fired, UI-visible recovery

### Phase 4 — D3 hub-client connection policy
- [ ] Health-arbitrated peer wait in preview-runtime/hub-client
      (q2-preview Phase 2 pattern), IndexedDB render fast-path intact
- [ ] hub-client `npm run build:all` + test:ci; manual browser check
      per the e2e policy (cold profile: documents open without
      "offline mode" flash)

### Phase 5 — rollout
- [ ] Full verify; deploy via quarto-hub-deployment (workflow re-run
      FIRST — see bd-erf hazards); colleague confirmation
- [ ] Close out bd-p68lx71t with the final root-cause narrative;
      unblock the audience re-rollout (bd-exs follow-up)

## Open questions for Carlos

1. **Hub-side accept policy** (if Phase 1 lands on (b)): should the
   hub accept any announced document from an authenticated client
   (current trust model: allowlisted user = trusted collaborator), or
   do we want index-membership validation while we're in there?
   Recommendation: accept-any for now, matching the existing model;
   validation is a separate hardening discussion.
2. **Part 3 budget**: how long may a cold hub-client session wait for
   the peer before declaring true offline? q2-preview uses
   health-arbitrated indefinite-with-arbitration; a simpler "5 s if
   health says reachable" may be enough for hub-client v1.
3. **hello-claude.qmd**: recover (needs the creating profile + the
   Part 1 fix) or recreate-and-clean? If the content was throwaway,
   recreate is zero-effort once the scan tool exists.
