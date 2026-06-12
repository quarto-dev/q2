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

### Phase 1 — localize D1 (COMPLETE 2026-06-12; verdict below)
- [x] test-hub harness with deferred-upgrade switch
      (`quarto-sync-client/src/test-hub.ts`)
- [x] Repro tests, three shapes, ALL GREEN in node:
      fresh-create against held JS hub; fresh-create against
      late-starting Rust hub; **restart-window** create (connect
      online → SIGKILL hub → createFile in the gap → hub returns on
      same port/data-dir → doc arrives). The announce machinery is
      **exonerated on both sides** — kept as regression guards
      (`offline-creation.test.ts`, `offline-creation-rust-hub.test.ts`,
      `restart-window-creation.test.ts`).
- [x] **VERDICT — hypothesis (c): a browser durability race, not an
      announce gap.** hub-client creates files through the exact
      sync-client path proven green above; the environmental delta is
      tab lifecycle. automerge-repo persists documents via an
      async-throttled save with a **100 ms debounce**
      (`Repo.js: saveDebounceRate = 100`); a document created during
      an offline window whose throttled IndexedDB write hasn't landed
      when the tab unloads (hard refresh — exactly what users do
      during an outage) loses its bytes entirely: the network can't
      save it (offline) and the disk never had it. The index entry
      survives (existing doc, separate flush/sync timing) — minting
      precisely the hello-claude dangling entry. Consistent with every
      observation, including the creator profile never self-healing
      after reconnect (there is nothing left to announce).

**Correction (Carlos, 2026-06-12, after Phase 1):** the browser
console repro was NOT the project index failing — the error
`Document automerge:3HJoqsM… is unavailable` is the *hello-claude
file document* (the message format invites the confusion; the
project index `SNHcgVzU…` loads fine). With that identification the
incident chain completes:

1. one file document lost at creation (D1; durability hypothesis
   below stands as the best candidate);
2. its index entry synced everywhere;
3. **one dangling entry bricks the whole project for every client**:
   `loadFileDocuments` (connect path) and `syncWithFiles`
   (index-change path, hitting already-open sessions) await `findDoc`
   per file with no error tolerance — the first unavailable doc
   throws out of `connect()`. Same defect the MCP server showed
   (bd-vm5e5u10), now promoted: it is the amplifier that turns one
   lost file into "the project fails to load" for the whole team.

**Priority re-order**: graceful degradation (bd-vm5e5u10, one fix in
sync-client serving browser + MCP) is now the highest-leverage item —
shipping it unbricks affected projects everywhere with no production
mutation. Then D1 durability (prevents new mintings), doctor
(blast-radius + standing health check), D2/D3 as planned.

**Recorded latent-risk note (evidence-downgraded, kept honest):**
while chasing a wrong hypothesis we found that `findDoc`'s retry loop
bails immediately when `connectedPeers.size === 0` (added in
e326eb5c, bd-jit6pdwq Phase 1). For cold-cache boots this converts
"slow but successful" (retry until the peer arrives) into "instant
fail" — it was NOT the incident cause (the failing doc genuinely
doesn't exist), but it deserves a test + fix in the D2 work: "no
peers *yet*" (booting) and "no peers" (offline) are different states.

**Second correction (2026-06-12, evening) — the actual casualty
identified and resolved:** the dangling entry was
`/cscheid/q2-mcp-hello.qmd` (created by the bd-81cfshmw live write
test at 15:31Z), not hello-claude.qmd. Its cause is NOT the browser
durability race below — it is the **MCP server exit racing outbound
sync** (stdin-EOF shutdown ran before the new doc reached the hub;
filed as bd-10deu8h4, the exit-flush race noted-and-deferred during
bd-xnmd5ni1). The playground was surgically unbricked (entry removed
with prior backup, Carlos-approved Path B; bd-8x482xb0 closed), the
audiences flag re-enabled after exoneration, and hello-claude.qmd —
never actually lost, just hidden behind the brick — edited normally.

The browser durability hypothesis below is hereby DEMOTED to a
latent concern with **no known casualty**: the 100 ms save debounce ×
tab unload window is real and the flush-on-create fix remains
worthwhile, but it is no longer load-bearing for any observed loss.
Part 1 therefore splits per host:
- **MCP host (bd-10deu8h4)**: drain outbound sync before exit and/or
  await server receipt in create/write handlers (needs a delivery
  signal — remote-heads gossip or an explicit settle; design in that
  strand);
  - **DONE 2026-06-12** (`2246e865` + `194b9cc3` on
    `beads/bd-10deu8h4-hub-mcp-server-exit`): shutdown drains via
    `disconnect({drainMs: 3000})`; delivery signal is per-doc
    remote-heads sync info keyed by the hub's handshake storageId
    (no gossiping flag needed; verified against real samod, both as
    a gated regression test and via manual `q2 mcp` e2e). Bounded,
    early-return, loud stderr on failure. Details + e2e record:
    `claude-notes/plans/2026-06-12-mcp-exit-sync-drain.md`. The
    per-write-receipt variant (tool handlers returning only after
    server confirmation) was deliberately left out per the
    bd-vm5e5u10 boundary contract — drain-on-shutdown covers the
    incident; file a follow-up strand if per-write UX is wanted.
- **Browser host**: flush-on-create + unload flush as below
  (defense-in-depth, playwright-verified).
Fix order remains: bd-vm5e5u10 (amplifier) → bd-10deu8h4 (creator)
→ doctor → D2/D3.

> **Amplifier FIXED (2026-06-12, bd-vm5e5u10 closed):** one dangling
> entry no longer bricks a project. Sync-client tolerates unavailable
> file docs (connect + index-change paths; the fire-and-forget
> `syncWithFiles` unhandled rejection is handled now), surfaces them
> via `status: 'unavailable'` / `onFileUnavailable` /
> `getUnavailableFiles()`; index-unavailable stays fatal with a
> message that says "index". MCP lists ghosts, gives per-file errors,
> and `delete_file` on a ghost is the self-service repair (verified
> e2e against a real hub). Details + transcript:
> `claude-notes/plans/2026-06-12-graceful-dangling-entries.md`.
> Retry-on-peer-arrival for unavailable files remains D2 scope. Also filed along the way: bd-3g0aijb3 (/auth/actor
+ /auth/me reject Bearer → MCP attribution silently degraded).

**Part 1 fix design, amended by the verdict** (supersedes the
re-announce sketch): make creation durable, not re-announced —
- sync-client `createFile`/`createNewProject` await
  `repo.flush([docId, indexDocId])` (API exists:
  `Repo.flush(documents?)`) before resolving, so "created" means
  "persisted locally" — cheap in node/memory, the whole point in the
  browser;
- hub-client adds a `beforeunload`/`visibilitychange` flush so even
  mid-debounce edits get a last write;
- browser-level confirmation test (playwright, hub-client suite):
  create offline + immediate reload → file must survive in IndexedDB
  (red today, green after).
The re-announce machinery needs no change (proven); the regression
guards stay.

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

## Resolved questions (Carlos, 2026-06-12)

1. **Hub accept policy**: accept-any from authenticated clients,
   matching the existing trust model (allowlisted user = trusted
   collaborator). A real authorization layer is acknowledged future
   work — it requires coordinating with external authorization
   systems where quarto-hub deployments embed in other auth setups —
   and is explicitly out of scope here.
2. **Part 3 budget**: "5 s if /health says reachable" approved for
   hub-client v1.
3. **hello-claude.qmd**: throwaway content, BUT the inconsistent
   storage state is deliberately **preserved as a live diagnostic
   fixture** while this work is in flight (playground project =
   internal-use, not production-critical). Do not clean it up until
   the D1 fix + doctor are done with it.

**Phase 1 design refinement** (from resolution discussion): the
localization red test runs against BOTH hubs — the in-process JS
automerge-repo harness AND the real Rust `hub` binary. If the JS run
reproduces the loss, the gap is client-side ((a)); if JS passes but
Rust fails, it's samod's accept path ((b)). The Rust-hub variant
simulates the offline window by starting the hub AFTER the client
created documents (adapter retry-loop reconnect = exactly the
production restart scenario).

## Open questions for Carlos

(none currently)

<details><summary>Resolved 2026-06-12 (see above)</summary>

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

</details>
