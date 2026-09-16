# Automerge index-document staleness — research notes

**Status:** INVESTIGATION ONLY, mechanisms now characterized (2026-09-16,
same day). H1 and H2 are both confirmed as *real, manufacturable*
mechanisms against automerge-repo v2.5.6 — see "Characterization results"
below. This still does **not** diagnose Gordon's specific incident: a
manufactured repro proves a mechanism is possible and matches the symptom
shape, not that it's what happened to his project. No fix has been
attempted; that still needs Gordon's go-ahead per "Next steps."

**Strand:** bd-6f21d4c6 ("Reproduce leading mechanisms of Automerge
index-document staleness bug").

**Worktree:** `.worktrees/workspace-2`, branch `explore/automerge-index-resync`.

## The symptom, in corrected vocabulary

A hub-client user's local copy of a project's `IndexDocument` (the
per-project Automerge root doc mapping file path → Automerge doc id;
`ts-packages/quarto-automerge-schema/src/index.ts:58-64`, tracked as
`indexDocId` throughout `quarto-sync-client`) stops receiving updates that
other collaborators' clients have already applied — e.g. a new or renamed
file. The user's Monaco editor then shows Q-13-4 ("body link references
missing document") squiggles on links to files that genuinely exist,
because the render pipeline is doing a real lookup against a `files` map
that is, for this one client, behind.

This is **not** a CRDT "fork" (no concurrent-edit divergence is implied —
nothing contradicts anything). It presents as **one client's replica going
inert**: connected, apparently online, but no longer receiving further
sync traffic for (at least) the index document. Gordon's word "unlinked" is
closer than "forked."

The only known workaround is clearing the `documents` object store in the
`automerge` IndexedDB database (Application → IndexedDB in Chrome devtools),
which forces a full clean resync. Reconnecting or reloading the page alone
has not been confirmed to fix it (Gordon has not been able to test this
distinction directly, since he can't reproduce on demand — it's inferred
from "clearing IndexedDB is the fix I reach for").

The symptom is long-lived — noticed emerging **over weeks**, not within a
session — and most often reported against "meeting notes" style documents:
long-lived files edited collaboratively by several people over time. This
is likely because such documents make *any* index staleness visible fastest
(many editors, frequent renames/moves, frequent cross-links to them), not
because the underlying mechanism is specific to that file's content.

### Terminology: "stranded peer registration"

Adopted term for the end state, settled 2026-09-16 after H1 tier 1's
confirmation pinpointed exactly where the bad bookkeeping lives: a
**stranded peer registration**. The client's transport layer
(`NetworkSubsystem`) believes the hub is connected and correctly routes
messages to it; the sync layer (`CollectionSynchronizer` / each affected
`DocSynchronizer`) has no record of that peer for a given document, so it
never initiates or resumes sync for it. Two words deliberately avoided:

- **"dangling"** — already claimed, pointing the *opposite* direction:
  bd-8x482xb0/bd-vm5e5u10 use it for an index *entry* referencing a file
  doc that never arrived. This bug is an index *missing* an entry (or more
  generally, a document not receiving updates) that peers already have.
- **"forked"** — ruled out above; no CRDT divergence is implied.

**PeerId vs. storageId — two different stability guarantees, matching H1
vs. H2.** Checked directly against the vendored `samod` (Rust) source that
becomes the production hub, not just inferred from the JS test harness:

- `samod-core/src/peer_id.rs`'s own doc comment: *"Peer IDs are ephemeral
  identifiers that identify a specific instance of a peer (e.g., a browser
  tab, a **process**)... different from storage IDs which identify the
  underlying storage."*
- `samod/src/lib.rs:1330`: `PeerId::new_with_rng(&mut rng)` runs **once**,
  when the `Repo`/`Samod` instance is built — once per hub **process**, not
  once per client connection. `quarto-hub`'s server startup never calls
  `RepoBuilder::with_peer_id(...)` to override it, so the hub's peerId is a
  random string minted at process start and held fixed for that process's
  entire uptime.

Consequence: despite automerge-repo being peer-to-peer in theory, in this
deployment every client dials into the same one hub process, and that hub
presents **one stable peerId to every client, for as long as the hub
process stays up**. That stability is not incidental to H1 — it's the
precondition: `CollectionSynchronizer.addPeer`'s dedup guard only misfires
because the peerId a reconnecting client sees really is the same value as
before. If the hub minted a fresh peerId per connection, every reconnect
would look like "a new peer arrived" and the guard would never trigger.
Given the actual architecture, this is a standing risk for any client
session that outlives even one reconnect blip against that hub, not a
rare coincidence.

`storageId` (what H2's throttle and the persisted `[docId, "sync-state",
storageId]` entries are keyed on) sits on a *more* durable axis than
peerId — it identifies the hub's underlying storage and can survive a hub
process restart if the storage backend does (production runs on
EBS/S3-backed storage; see `CLAUDE.md`'s local-prod notes). PeerId cannot
survive a restart (a fresh process mints a fresh random one). This also
explains why the IndexedDB-wipe workaround works without anyone having
known which axis actually mattered: it discards the storageId-keyed
persisted state *and*, by forcing a page reload, mints a fresh client-side
`Repo` (and therefore fresh peerId-scoped bookkeeping) — clearing both
axes at once.

## Sources now available for this investigation

Symlinked into `external-sources/` (matching the existing `pandoc` /
`quarto-cli` convention) during this session:

- `external-sources/automerge` → `~/src/automerge` (automerge core, Rust +
  JS/WASM; checked out at tag `rust/automerge@0.10.0`)
- `external-sources/automerge-repo` → `~/src/automerge-repo` (the JS
  `Repo`/`DocHandle`/sync-protocol library hub-client actually runs; cloned
  fresh this session and checked out at tag `v2.5.6`, matching hub-client's
  installed `@automerge/automerge-repo` version exactly — see
  `hub-client/package.json`)
- `external-sources/samod` → `~/src/samod` (Rust reimplementation of
  automerge-repo, used server-side by `crates/quarto-hub`; v0.12.0)

All three are read-only reference checkouts per the repo's External Sources
Policy (`CLAUDE.md`) — never referenced from compiled code or build scripts.

## What the workaround tells us

The IndexedDB `automerge` database has a single object store, `documents`
(`automerge-repo-storage-indexeddb/src/index.ts:12-14` — Gordon's phrase
"documents table" is exactly the real name). Automerge-repo stores **both**
document change-chunks *and* the persisted per-peer sync protocol state in
this one store, keyed by array:

- doc bytes: `[documentId, "snapshot" | "incremental", hash]`
- sync state: `[documentId, "sync-state", storageId]`
  (`StorageSubsystem.ts:322-343`)

`storageId` here is the **remote peer's stable identity** (announced once
in its connection handshake metadata), not the ephemeral per-connection
`PeerId` used for a single WebSocket session. This persistence is
deliberate: it lets a reconnecting client skip re-sending full history to a
peer it has already synced with, by resuming from where it left off.

Wiping the whole store discards this persisted sync state along with the
doc chunks, forcing every document to start syncing from
`A.initSyncState()` — a full clean handshake — on the next connect. **This
means the workaround's mechanism is consistent with the bug being "a client
believes it is already synchronized with a peer/document when it is not,"**
whether that belief lives in the persisted sync state or in some other piece
of session bookkeeping that a full reload does not reset but wiping storage
incidentally does (e.g. by forcing `Repo`/`DocHandle` recreation with a
guaranteed-clean slate).

## Ruled out (checked directly against automerge-repo v2.5.6 source)

**A corrupted/undecodable persisted sync-state blob does *not* explain a
permanent wedge.** `StorageSubsystem.loadSyncState` (`StorageSubsystem.ts:
322-334`) wraps both the storage read and `A.decodeSyncState` in its own
try/catch and returns `undefined` on any failure — which
`DocSynchronizer.#withSyncState` (`DocSynchronizer.ts:148-158`) then treats
as "no prior state," seeding `A.initSyncState()` (a fresh full resync) via
`#initSyncState`. A corrupted stored blob should *self-heal* on the very
next connection attempt, not require a manual IndexedDB wipe. (The outer
`.catch` in `DocSynchronizer.ts:154-156`, which only logs, is effectively
dead code for this failure mode given `loadSyncState`'s own handling — it
would only matter if `onLoadSyncState` itself threw synchronously, which it
doesn't.)

**An already-fully-synced (`ready`) document handle cannot be pushed into
`unavailable`** (and therefore cannot have its in-memory content reset to
`A.init()` — see `DocHandle.ts:110-112`, `onUnavailable`) by
`DocSynchronizer.#checkDocUnavailable` (`DocSynchronizer.ts:413-440`): that
method's guard requires `this.#handle.inState([REQUESTING, UNAVAILABLE])`,
which is false once a handle has ever reached `ready`. This mechanism (real,
and worth remembering for a *different* bug shape) only threatens documents
that are still loading for the first time — i.e. it's cold-start territory,
already covered by bd-10bdjmjb's D1/D2, not "goes stale after working fine
for weeks."

## Testability — can we manufacture these deliberately?

Short answer: **yes for H1 and H2**, without waiting for organic
reproduction — both depend on event/timing orderings that a Node test can
fully control, using infrastructure that already exists in this repo. **No**
for confirming which mechanism actually caused any *specific* real incident
— that still needs telemetry from a live occurrence (see Next steps). A
manufactured repro can prove a mechanism is *possible* and matches the
symptom shape; it can't prove it's what happened to Gordon's project without
corroborating evidence from that project's own storage/logs.

Existing pieces that make this cheap rather than needing new harness code:

- `ts-packages/quarto-sync-client/src/test-hub.ts`'s `startTestHub()` — a
  real in-process automerge-repo hub (Node `ws` server + a real `Repo`),
  with a `holdUpgrades` / `releaseUpgrades()` knob already built to hold a
  websocket upgrade until the test says go (built for bd-10bdjmjb's
  cold-start races — the same shape of "control exactly when the peer
  handshake lands" we need here).
- `MemoryStorageAdapter` (`ts-packages/quarto-sync-client/src/storage-
  adapter.ts`) — a plain in-memory `StorageAdapterInterface`, already used
  by both `test-hub.ts` and q2's own client. Handing a test's own instance
  to a client-side `Repo` and spying on its `save()` calls gets a
  deterministic, IndexedDB-free record of exactly which storage keys
  (including `[docId, "sync-state", storageId]`) actually get written —
  no fake IndexedDB needed.
- `automergeSync.getRepo()` / `getDocInventory()` (bd-q93tkglb's debug
  surface) — already exposes the live `Repo` for observation; useful for
  asserting internal peer/sync state from outside without adding new
  production code.
- `createSyncClient()` — the exact same client class hub-client runs in
  the browser, already used directly (not mocked) by
  `ts-packages/sync-test-harness`'s existing tests
  (`concurrent-editing.test.ts`, `roundtrip.test.ts`). A new test in that
  package or in `quarto-sync-client` itself is the natural home.

### H2 — testable now, and the mechanism itself needs no fault injection

This one doesn't require racing anything — `asyncThrottle`'s "latest call
always runs, canceling previous pending calls" behavior
(`automerge-repo/src/helpers/throttle.ts`) is unconditional, confirmed by
reading the code (not just plausible). A test can demonstrate the drop
directly:

1. Start a `TestHub`. Connect one real `SyncClient` to it and load/create an
   index doc plus 2+ file docs so all of them sync sync-state with the same
   `storageId`.
2. Replace (or spy on) the client-side storage adapter's `save()` so the
   test can see every `[documentId, "sync-state", storageId]` key that
   actually gets written.
3. Use `vi.useFakeTimers()` (already a project dependency via vitest) to
   control the 100 ms `#saveDebounceRate` window precisely. Trigger a
   `sync-state` event for doc A, then within the same throttle window
   trigger one for doc B, then advance fake time past the debounce.
4. Assert: doc A's sync-state save never happened; only doc B's did. That's
   the mechanism, mechanically proven, in well under an hour of test-writing
   — no real network or real IndexedDB involved.

This proves the *drop* is real. It does **not** by itself prove permanent
staleness (per the ruled-out reasoning above, a dropped persist is normally
just a slower catch-up) — pair with a second assertion that the sync
protocol still converges correctly on the next real message exchange, to
keep the "compounding factor, not standalone cause" framing honest.

### H1 — testable, in two steps of increasing realism

**Step 1 (cheap, fully deterministic — tests automerge-repo's contract
directly):** `CollectionSynchronizer.addPeer`'s dedup guard
(`if (this.#peers.has(peerId)) return`) can be exercised without any real
socket. Construct a bare `Repo` with a hand-written fake `NetworkAdapter`
(the interface is small: `connect`/`send`/`disconnect` plus emitting
`peer-candidate`/`peer-disconnected`/`message`) — a test fully authors the
event sequence itself, so there is nothing to race:
1. Open (`repo.find`/`repo.create`) a document, `emit('peer-candidate', {peerId: 'X', ...})`,
   let it reach `ready`.
2. `emit('peer-candidate', {peerId: 'X', ...})` **a second time, with no
   intervening `peer-disconnected`** — simulating the exact ordering
   violation H1 describes.
3. Assert whether `docSynchronizer.beginSync` fires again for peer X (e.g.
   by spying on the message-emission the synchronizer would otherwise
   produce, or exposing `CollectionSynchronizer.docSynchronizers[id].hasPeer('X')`
   through the debug accessor).

This settles a factual question we currently don't know the answer to:
*does automerge-repo's own dedup guard actually suppress a resync when
this ordering happens, or does something else in the pipeline (e.g.
`reevaluateDocumentShare`, or a change in a later automerge-repo patch
version than the one archived here) already cover it?* That's answerable
today, from the vendored source, without waiting for anything.

**Step 2 (more realistic, still controlled, but not fully deterministic):**
use `startTestHub({ holdUpgrades: true })` with a *real* `SyncClient` and
force an overlap between the old socket's `close` and the new socket's
`open` by driving the client's underlying WebSocket lifecycle directly
(reachable via `getRepo()`'s `networkSubsystem`) rather than waiting for
the library's own 5s retry timer. This is closer to production but Node's
real socket/task-queue timing means the exact interleaving can't be forced
with 100% certainty the way Step 1's fake adapter can — treat it as a
confirming test after Step 1 establishes the mechanism is real in
principle, not as the primary evidence.

### H3 — not a repro target

H3 was a static "did we leave a leak" check, not a timing/ordering
hypothesis, so there's nothing to manufacture a race for. If more
confidence is wanted, the only testable statement is a regression-shaped
one: after `connect()` → `disconnect()` → `connect()` (repeated a few
times), assert exactly one live `Repo`/adapter pair exists and the old
ones' event listeners never fire again. That's a straightforward test to
add but it isn't a *reproduction* of anything — it would only catch a
regression if this ever breaks in the future, not confirm or deny a cause
of Gordon's symptom today.

## Characterization results (2026-09-16)

Both H1 tier 1 and H2 were exercised per the recipes above. Both are now
**confirmed, mechanically, against the vendored automerge-repo v2.5.6
source** — not just plausible from reading it. Tests are characterization
tests (they pass against current upstream behavior; they are not
fix-driving red tests) in `ts-packages/quarto-sync-client/src/`:

- **H2 — CONFIRMED.**
  `sync-state-save-throttle.test.ts`: a real `SyncClient` against a real
  `startTestHub()`, with `vi.useFakeTimers()` controlling the 100ms
  `#saveDebounceRate` window and a `vi.spyOn(MemoryStorageAdapter.prototype,
  'save')` recording every storage write. Firing `sync-state` for doc A then
  doc B within the same 100ms window: doc A's `[docId, "sync-state",
  storageId]` save never happens; only doc B's does. A sanity variant
  (events spaced past the window) confirms both saves happen when the
  window doesn't overlap — the test discriminates the mechanism, it isn't
  vacuously green. A second assertion (real edit to doc A's content,
  polled against the hub's copy) confirms the sync protocol still
  converges afterward — the drop is a storage-persistence effect only;
  `DocSynchronizer`'s live in-memory sync state is set *before* the
  `sync-state` event is emitted (`DocSynchronizer.ts`'s `#setSyncState`),
  so it's untouched by the dropped persist. This keeps the original
  "compounding factor, not standalone cause" framing honest.
- **H1 tier 1 — CONFIRMED, and it's worse than "plausible."**
  `collection-synchronizer-peer-dedup.test.ts`: a hand-written fake
  `NetworkAdapter` drives a bare `Repo` directly. A first `peer-candidate`
  for a peer starts `beginSync` on every registered document (spied
  directly on the `DocSynchronizer` instance); a **second** `peer-candidate`
  for the *same* `peerId`, with no intervening `peer-disconnected`, calls
  `beginSync` **zero** additional times — the dedup guard's early return is
  real, not covered by anything else in the pipeline. A control case (an
  intervening `peer-disconnected` before the second `peer-candidate`) shows
  `beginSync` firing normally, isolating that the suppression is
  specifically about the missing-disconnect ordering, not something else.
  Reading `NetworkSubsystem.addNetworkAdapter` while building the fake
  adapter turned up something the original hypothesis didn't know: its
  `peer-candidate` handler (`NetworkSubsystem.ts:52-66`) unconditionally
  re-emits `"peer"` to `Repo` on *every* `peer-candidate`, including
  repeats for an already-connected peer — with the library's own
  acknowledged `// TODO: on reconnection, this would create problems!`
  comment sitting right above it. So `CollectionSynchronizer.addPeer`'s
  guard is the *only* place in the whole pipeline that could catch this,
  and it doesn't. This raises H1 from "plausible, sibling to a known fixed
  bug (bd-jit6pdwq)" to "a second, distinct, currently-unfixed defect in
  the same reconnect-ordering neighborhood, confirmed by direct
  experiment."

Both tests are green (they characterize real, current behavior, not a
regression), pass `npx tsc --noEmit` in `ts-packages/quarto-sync-client`,
and the package's full suite (140 tests) is otherwise unaffected.

**What is still not settled:** whether either mechanism (or H1 specifically,
now that it's a confirmed defect) is *the* cause of Gordon's actual
incident. Manufacturing the mechanism proves it's real and matches the
symptom shape; it doesn't prove causation for any specific past occurrence
without corroborating evidence from that occurrence (see "Next steps",
unchanged). H1 tier 2 (a real-socket confirming test forcing an actual
reconnect overlap) has not been attempted — per the handoff, that and any
production fix wait on Gordon's decision now that tier 1 has settled the
factual question tier 2 was gated behind.

## Leading hypotheses, ranked

### H1 (most plausible, and — as of 2026-09-16 — tier-1-confirmed): a peer add/remove ordering race silently skips resync for an already-open handle

`CollectionSynchronizer.addPeer(peerId)` (`CollectionSynchronizer.ts:
179-193`) is a no-op if `peerId` is already in its `#peers` set — it assumes
`removePeer` (fired from `peer-disconnected`) always completes *before* the
next `addPeer` (fired from `peer-candidate`/`peer` on reconnect) for the same
logical peer. If that ordering is ever violated — e.g. two socket lifecycles
briefly overlapping across a reconnect — `addPeer` returns early and **never
calls `beginSync` on any of the currently-registered `DocSynchronizer`s for
that peer**. Nothing else ever retries this for an already-registered,
already-"open" document/peer pair; the index document (uniquely, the one
document held open for a session's *entire* duration, vs. per-file handles
that churn) is the one most exposed to ever hitting this once, and once hit,
stays hit until something recreates the `Repo` from scratch (a full
reconnect cycle with a genuinely fresh peer identity, or the workaround).

This is not a hypothetical class of bug in this exact code path: q2 has
already found and patched a **sibling** race here —
`StoppableWebSocketClientAdapter`
(`ts-packages/quarto-sync-client/src/StoppableWebSocketClientAdapter.ts`)
exists specifically because upstream automerge-repo 2.5.6's
`WebSocketClientAdapter.disconnect()` doesn't cancel the `onClose`-scheduled
reconnect timer, so a "torn down" adapter could resurrect itself and retry a
dead connection forever (bd-jit6pdwq). That the reconnect lifecycle in this
exact library version already had one real, shipped defect raises the prior
probability of a second, not-yet-found one nearby.

**What would confirm this:** instrumentation logging
`CollectionSynchronizer`'s `#peers` set size and every `addPeer`/`removePeer`
call (peerId, and whether it was a no-op) across a long-lived session, then
correlating a "files list froze" report against a same-peerId `addPeer`
no-op that fired without an intervening `removePeer`.

### H2 (confirmed as of 2026-09-16; plausible but weaker alone as a standalone cause): per-storage-id sync-state persistence throttling can leave a stale-but-valid state for a specific document

`Repo.ts#saveSyncState` (`Repo.ts:371-403`) throttles persistence **per
remote `storageId`**, not per document, via `asyncThrottle`
(`this.#saveDebounceRate`, default 100ms). One busy multi-document session —
exactly what a collaboratively-edited "meeting notes" file plus its
neighbors in the same project produce — can generate sync-state-changed
events for several different documents faster than the shared per-peer
throttle drains them, so some documents' persisted sync state can lag
behind what was actually last exchanged with that peer.

On its own this likely only causes a *slower* catch-up next session (the
sync protocol is designed to converge from any starting sync state given
continued message exchange), not a permanent wedge — so this is offered as
a **contributing/compounding factor**, not a standalone explanation. It
becomes durable only if paired with something else (most plausibly H1)
that prevents any further message exchange from ever nudging that
document/peer pair forward again.

**What would confirm this:** comparing a stuck client's persisted
`sync-state` IndexedDB entry (decoded) against the actual current hub-side
heads for that document, at the moment the bug is caught in the wild —
would show a real but recoverable-in-principle gap, not corruption.

### H3 (checked, not currently supported): a leak across `connect()`/`disconnect()` calls in q2's own `SyncClient`

Checked directly: `disconnect()` (`client.ts:1238-1286`) does null out
`state.repo`, `state.indexHandle`, the WS adapter (calling its own
`.disconnect()`), and all `state.cleanupFns`-registered listeners before
`connect()` constructs a fresh `Repo`. No obvious cross-call leak found.
Kept here only as a note that it was checked and looked clean, so a future
investigator doesn't re-walk this same path expecting to find something.

## Prior, related work (not the same bug)

- **bd-10bdjmjb** ("Browser sync offline-fallback race family", design at
  `claude-notes/plans/2026-06-12-sync-client-offline-race.md`, still
  DESIGN/no-implementation-yet) — covers three defects (D1: created-while-
  offline documents never reaching the hub; D2: opens that failed during the
  1ms-peer-wait fallback never retrying once the peer connects; D3: the 1ms
  default itself). All three are about the **cold-start / initial-open**
  window, not an already-open, already-synced document going stale mid- or
  cross-session. Worth re-reading in full before any fix work, since D2's
  self-heal-on-reconnect design (Part 2 of that plan) is adjacent territory
  and any fix here should stay consistent with it.
- **"Dangling index entry"** (bd-8x482xb0, closed; bd-vm5e5u10, closed) — the
  established house term, but for the *opposite* direction: an index entry
  pointing at a file document that never reached the hub. This bug is the
  reverse: an index *missing* an entry that other peers already have.
- **q2 debug surface already exists for this class of problem**:
  `getRepo()` / `getDocInventory()` in `automergeSync.ts:311-333`
  (`quartoDebug.am`, bd-q93tkglb) exposes the live `Repo` and a per-document
  inventory for observation. Extending this surface (peers per document,
  persisted-vs-live sync state) is the natural instrumentation seam — see
  Next steps.

## Next steps (only if/when this becomes actionable)

1. **Instrument, don't guess.** Add optional, env/flag-gated logging (or
   extend `getDocInventory()`) that records, per document: the peer set
   `DocSynchronizer` believes it has, and whether `addPeer` ever no-op'd
   against a peer already believed connected. This is cheap, low-risk, and
   turns the *next* occurrence into a real repro instead of another
   "clear IndexedDB and move on."
2. **Ask affected users to export before clearing.** `braid`/support
   channel: before anyone runs the IndexedDB-clear workaround again, ask
   them to export the `documents` store first (devtools can do this, or a
   small bookmarklet). A corrupted-vs-merely-stale persisted sync-state
   entry is trivially distinguishable once we have one in hand, and would
   immediately confirm or kill H2.
3. **Do not attempt a fix from this doc alone.** Per CLAUDE.md's
   TDD requirement, any fix needs a red test first. Per "Testability"
   above, H1 and H2 *can* be exercised deliberately today (existing
   `test-hub.ts` / `sync-test-harness` / `MemoryStorageAdapter` cover the
   needed control surface) — that gets us a red test for "this mechanism
   is real," which is necessary but not sufficient. What we still cannot
   manufacture is confirmation that a given mechanism is *the* cause of
   any specific real incident; that piece stays gated on organic
   reproduction plus step 1's instrumentation, honestly.
