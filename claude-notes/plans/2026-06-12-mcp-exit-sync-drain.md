# bd-10deu8h4: MCP server exit must not race outbound document sync

**Strand:** bd-10deu8h4 (p1). Related: bd-8x482xb0 (closed — the
production casualty this caused), bd-p68lx71t (the 2026-06-12
incident), bd-vm5e5u10 (the amplifier, being fixed IN PARALLEL — see
boundary contract below), bd-xnmd5ni1 (closed — requireOnline, which
ensured the *connection* but not *delivery*), parent plan
`claude-notes/plans/2026-06-12-sync-client-offline-race.md`.
**Status:** IN PROGRESS (2026-06-12, worktree
`.worktrees/bd-10deu8h4-hub-mcp-server-exit`, branch
`beads/bd-10deu8h4-hub-mcp-server-exit` off `4bd6d4bf`). Designed for
parallel implementation alongside bd-vm5e5u10.

## Work items

### Phase 1 — red tests + delivery-signal investigation

- [x] Delivery-signal investigation; verdict recorded below
      (§ Phase 1 verdict).
- [x] Additive API surface so the red tests compile:
      `DisconnectOptions`/`DisconnectReport` in sync-client `types.ts`,
      `disconnect(options?)` signature stub (NO drain behavior yet),
      `MemoryStorageAdapter` exported from sync-client index.
- [x] Test hubs announce a `storageId` like the real samod hub does:
      give both `test-hub.ts` copies a `MemoryStorageAdapter`; add
      `hubHasDoc` to the hub-mcp copy. (Also: their `stop()` now
      tolerates `repo.shutdown()`'s flush throwing "DocHandle is not
      ready" — the half-delivered state these tests leave behind.)
- [x] Red test 1 (sync-client `exit-drain.test.ts`):
      create-then-disconnect(drainMs) loses nothing; observed RED.
- [x] Red test 2 (hub-mcp `exit-drain.test.ts`, stdio level): the
      exact accident — `create_project` → stdin EOF → server exits AND
      hub holds the file doc; observed RED.
- [x] Red-output snippets recorded in this plan (§ Phase 1 red
      evidence).

### Phase 1 red evidence (2026-06-12, before any fix)

Note on red-shape tuning (predicted by this plan): with 4×64 KB files
the stdio test was GREEN by luck — on loopback, the `create_project`
response round-trip gives the event loop time to flush outbound sync.
At 64×64 KB (more docs = per-doc synchronizer backlog, more bytes =
encode/send time) it is deterministically red. The sync-client-level
test is red even at 8×64 KB because `disconnect()` follows
`createNewProject()` with no intervening round-trip.

`quarto-sync-client`, `npx vitest run src/exit-drain.test.ts`:

```
FAIL src/exit-drain.test.ts > … > create-then-disconnect loses nothing
AssertionError: index doc must reach the hub: expected false to be true

FAIL src/exit-drain.test.ts > … > reports undelivered docs when the hub
is unreachable at disconnect
AssertionError: drain cannot succeed against a dead hub: expected true
to be false
```

`quarto-hub-mcp`, `npx vitest run src/exit-drain.test.ts` (server exits
within the 5 s hygiene bound — that assertion passes — but file docs
died with the process; the index escaped, exactly the incident):

```
FAIL src/exit-drain.test.ts > … > create_project then immediate stdin
EOF must not lose the created docs
AssertionError: file doc for q2-mcp-hello-30.qmd must reach the hub —
its only copy was in-process: expected false to be true
```

### Phase 2 — implement

- [x] sync-client: drain primitive (`drainOutbound`) wired into
      `disconnect({drainMs})`; default 0 (browser teardown unchanged).
      Event-driven (`remote-heads` per handle + `peer` for mid-drain
      reconnects), bounded by `drainMs`, early-returns on confirmation.
- [x] sync-client: peer storageId tracking in `trackPeers` (kept after
      peer-disconnect — confirmed heads stay confirmed).
- [x] hub-mcp: `disconnectAll({drainMs})` + loud stderr on undrained
      projects (names indexDocId + paths; stderr only, bd-sl4o01y0).
- [x] hub-mcp: `index.ts` shutdown passes `SHUTDOWN_DRAIN_MS = 3000`;
      keeps re-entrancy guard; stdin-EOF exit stays within the 5 s
      hygiene bound (3 s budget binds only when the hub is gone).
- [x] Both red tests green; loud-failure path tested at BOTH levels
      (sync-client report assertions + stdio stderr assertion with
      prompt exit).
- [x] BONUS: real-samod drain regression test (gated on
      `target/debug/hub`, unix): create → `disconnect({drainMs})` →
      report.drained AND every doc present in samod's on-disk storage.
      This behaviorally verifies the delivery signal against the
      production hub implementation — passed 2026-06-12.
- [x] Suites green (2026-06-12): sync-client 99/99 (incl. rust-hub
      gated), hub-mcp 181 passed/3 skipped (incl. bundle test;
      skipped = keyring-gated e2e-auth), hub-client `npm run build` +
      `test:ci` 97/97. `cargo xtask verify --skip-hub-build
      --skip-hub-tests`: see Phase 3 note (run alongside).

### Phase 3 — verification + close-out

- [x] `cargo xtask verify --skip-hub-build --skip-hub-tests`: all
      steps passed (2026-06-12).
- [x] Manual e2e (2026-06-12, recorded below): rebuilt bundle + q2
      (`cargo xtask build-hub-mcp-bundle && cargo build --bin q2`;
      `--launcher-info` confirmed embedded gitCommit 194b9cc3),
      replayed the original accident against a LOCAL Rust hub, output
      inspected. Real-samod delivery-signal verification also locked
      in as a gated regression test (sync-client exit-drain).
- [x] Loud-failure path demonstrated once end-to-end (below).
- [x] braid: closed bd-10deu8h4 (fix 194b9cc3, merge c0e5e136); noted
      in parent plan; merged `--no-ff` into the integration line as
      second merger (one trivial doc-comment conflict in
      hub-mcp/test-hub.ts — both strands independently added
      repo + hubHasDoc; bodies were identical). Combined-state suites
      green: sync-client 105/105, hub-mcp 185/2-skipped, hub-client
      build + test:ci 97/97.

### Phase 3 manual-e2e record (2026-06-12)

Happy path — the exact production shape (one small file), through the
real binary a user runs, against `target/debug/hub` on 127.0.0.1:3041
with a fresh data dir:

```
$ node tmp-e2e/accident-replay.mjs ./target/debug/q2 tmp-e2e/hub-data \
    ws://127.0.0.1:3041/ws
created project 4XoG8ZeWkMCF6uJuu8RcojcY7u5N with 1 file(s)
server exited: true (8 ms after stdin EOF)
hub storage has 4XoG8ZeWkMCF6uJuu8RcojcY7u5N: true
hub storage has 3U9wPp6BomDhXUfkW66fvxiT8LgC: true
```

8 ms exit = the drain's first check found delivery already confirmed
(remote-heads from samod) and returned without waiting — the budget
binds only when something is actually undelivered. Both the index and
the file doc verified present in samod's on-disk storage (the artifact
the incident lost).

Loud-failure path — hub SIGKILLed mid-session, `create_file` during
the outage, then stdin EOF:

```
created project 1Yxc2e2zULCy6631LkhcVXVu2kR with 1 file(s)
killed hub pid 31167; creating a file during the outage…
server exited: true (3011 ms after stdin EOF)
--- server stderr ---
[disconnect] drain budget (3000 ms) expired with 2 possibly-undelivered
document(s): <index 1Yxc2e2zULCy6631LkhcVXVu2kR>, doomed.qmd
[hub-mcp] WARNING: exiting before outbound sync completed for project
1Yxc2e2zULCy6631LkhcVXVu2kR. Possibly NOT delivered to the hub (and
lost — this server keeps no local copy): <index document
1Yxc2e2zULCy6631LkhcVXVu2kR>, doomed.qmd. Verify these documents on
the hub before trusting them.
```

Bounded (3011 ms ≈ the 3000 ms budget; never hangs), loud (names the
project, the index — whose heads moved when the outage-created file
was indexed — and the path), and exit still well inside the 5 s
stdin-EOF promptness contract. The replay driver was a throwaway
script (`tmp-e2e/accident-replay.mjs`, deleted after use); the same
shapes are permanently covered by `quarto-hub-mcp/src/exit-drain.test.ts`
(JS hub, stdio level) and the samod-gated test in
`quarto-sync-client/src/exit-drain.test.ts`.

## Phase 1 verdict: delivery signal (RECORDED 2026-06-12)

**Chosen signal: per-document remote-heads sync info** —
`DocHandle.getSyncInfo(hubStorageId).lastHeads` equal to the handle's
current `heads()`, where `hubStorageId` comes from the hub's handshake
peer metadata. Drain = wait (bounded, event-driven via the handle's
`remote-heads` event) until every tracked doc (index + all file docs)
satisfies the equality against at least one storage-backed peer.

Why this is correct, from the sources (automerge-repo 2.5.6 installed;
samod q2 fork checkout `0b50c16`):

1. **No gossiping flag needed.** `Repo`'s constructor subscribes to the
   synchronizer's `sync-state` event and calls
   `handle.setSyncInfo(storageId, {lastHeads: theirHeads, ...})`
   unconditionally (`dist/Repo.js` ~154-175). The
   `enableRemoteHeadsGossiping` flag only gates *relay* of third-party
   heads (`remote-heads-changed` control messages), not this direct
   path. `theirHeads` comes from the automerge sync protocol: every
   sync message carries the sender's heads, so after the hub applies
   our changes its reply advertises heads that converge with ours.
2. **The signal is keyed by the peer's storageId from handshake
   metadata** (`peerMetadataByPeerId`); peers without a storageId never
   populate it. **samod always announces one**:
   `samod-core/src/actors/hub/state.rs::get_local_metadata()` returns
   `PeerMetadata { is_ephemeral: false, storage_id: Some(..) }`, and the
   wire protocol encodes it (`wire_protocol.rs`, `storageId` CBOR key).
   Source-verified on the q2 fork; *behavioral* verification against
   the real hub binary happens in the Phase 3 manual e2e (acceptance
   criterion) — do not close the strand before it.
3. **Awaitable**: `DocHandle.setSyncInfo` emits a `remote-heads` event
   (`dist/DocHandle.js`), so the drain early-exits the moment delivery
   is confirmed — no polling, no fixed sleep.
4. **Equality, not subset**: `UrlHeads` equality (order-insensitive) is
   the convergence steady state. If the hub holds *extra* changes, the
   live sync we're draining over delivers them to us within the same
   window, after which equality holds. Comparison re-reads
   `handle.heads()` on every event for exactly this reason.

**Degradation when the hub is unreachable**: no peer → no sync-state
events → drain waits out the bounded budget (the WS adapter's retry
loop may still reconnect mid-window and deliver — waiting is a feature),
then reports the undelivered docs; the MCP shutdown path prints a loud
stderr line naming the project and paths. Exit is never blocked past
the budget.

**Rejected alternatives**: sync-message settle heuristic (needs adapter
introspection, and "no outbound traffic for N ms" is consistent with
both "delivered" and "stalled"); verification re-find via a second
connection (correct but heavyweight, and it would double connection
churn at exit — keep as fallback if samod's heads behavior surprises us
in Phase 3).

**Test-hub consequence**: both JS test hubs construct `Repo` *without*
storage, so they announce no storageId (`storageId: await
storageSubsystem?.id()` → `undefined`) and the drain signal can never
fire against them — unlike the production samod hub. Phase 1 therefore
gives both test hubs a `MemoryStorageAdapter` (sync-client already
defines one) so their handshake metadata matches samod's shape.

**API decision (Phase 2 design)**: `disconnect(options?: { drainMs?:
number })` returning a `DisconnectReport { drained, undelivered }`;
default `drainMs: 0` — i.e. opt-in. Justification: hub-client's browser
`disconnect()` runs on tab/component teardown where blocking is
unacceptable AND harmless to skip (IndexedDB persists local changes;
they deliver on the next connect). The MCP server is the caller with
memory-only storage where exit = data loss, so it passes the budget
(3000 ms, comfortably inside the 5 s stdio-hygiene exit bound, early
exit on confirmation).

## Branch / coordination — READ FIRST

- Start from **`origin/feature/bd-81cfshmw-q2-mcp-launcher`** at
  `0fc9f2db` (NOT main — the sync-client groundwork and test harness
  live there, unmerged). Create your own topic branch off it; when
  done, merge back into that integration line with `--no-ff`
  (`.claude/rules/worktrees.md` § Integration-line convention).
- **Parallel-work boundary contract with bd-vm5e5u10** (in flight on
  the same integration line, plan:
  `2026-06-12-graceful-dangling-entries.md`):
  - THEY own: `client.ts` `loadFileDocuments` / `syncWithFiles` /
    `indexChangeHandler`, and `quarto-hub-mcp/src/tools.ts`
    (all tool handlers). **Do not edit those.**
  - YOU own: `client.ts` `disconnect()` (+ any new drain/delivery
    primitive you add near it), `quarto-hub-mcp/src/index.ts`
    (shutdown path), `quarto-hub-mcp/src/connection-manager.ts`
    (`disconnectAll`).
  - Shared, additive-only (merge-friendly): `types.ts` interfaces,
    `index.ts` exports of sync-client, new test files.
  - Whoever merges second resolves; with this split, conflicts should
    be trivial or absent.

## What happened (incident context, condensed)

On 2026-06-12 a `q2 mcp` session created
`/cscheid/q2-mcp-hello.qmd` on the production playground, read it
back **from process memory**, and exited when the driver closed
stdin. The stdin-EOF shutdown (bd-9jq2a060 — correct behavior, MCP
hosts terminate servers this way) ran
`manager.disconnectAll()` → `process.exit(0)` **before the new file
document's sync to the hub completed**. The index entry escaped (an
existing, already-synced document); the file document's only copy
died with the process (MCP clients use **memory storage** — there is
no local persistence to fall back on). Result: a dangling index entry
that bricked the project for every client (the bd-vm5e5u10 defect),
i.e. a production incident.

`requireOnline` (bd-xnmd5ni1) does not prevent this: it guarantees a
peer connection existed at *create* time, not that the created bytes
were *delivered* before exit.

## Required behavior

1. **Shutdown drains before exiting.** When the server shuts down
   (stdin EOF, SIGINT/SIGTERM, `server.onclose`), outbound document
   sync gets a bounded window to complete. Created-but-undelivered
   documents must reach the hub before `process.exit` whenever the
   connection allows it.
2. **Bounded, never hanging.** MCP hosts expect prompt termination
   (and `stdio-hygiene.test.ts` asserts exit within 5 s of stdin
   EOF — do not break bd-9jq2a060). Pick a drain budget that fits
   (suggestion: up to ~3 s total, returning EARLY the moment
   delivery is confirmed; adjust the hygiene test's bound only if
   justified, with a comment).
3. **Loud on failure, never silent.** If the budget expires with
   undelivered documents (hub unreachable, mid-restart), write a
   clear stderr line naming the project and paths/doc ids that may
   not have been delivered — the user/agent must be able to know.
   (stderr only — stdout is protocol; bd-sl4o01y0.)
4. The drain lives in `client.disconnect()` (sync-client) and/or
   `disconnectAll()` (connection-manager) + the shutdown path in
   `index.ts` — see boundary contract. A new public sync-client
   primitive (e.g. `whenDelivered(docIds?, {timeoutMs})` or
   `disconnect({drainMs})`) is yours to design in Phase 1.
5. **Out of scope** (boundary + follow-ups): per-write delivery
   confirmation inside tool handlers (`create_file` returning only
   after server receipt) — better UX but conflicts with
   bd-vm5e5u10's tools.ts ownership; file a follow-up strand if
   Phase 1 makes it cheap. Browser-side flush-on-create — parent
   plan. The doctor tool — parent plan Phase 1.5.

## Phase 1 — red tests + delivery-signal investigation

Red tests first (the accident, miniaturized). Harness:
`ts-packages/quarto-sync-client/src/test-hub.ts` (in-process hub with
`hubHasDoc(docId)` = server-side ground truth).

New `ts-packages/quarto-sync-client/src/exit-drain.test.ts`:
1. **create-then-disconnect loses nothing**: `createNewProject`
   (online, `requireOnline: true`, memory storage) → immediately
   `await c.disconnect()` → `hub.hubHasDoc(fileDocId)` must be true.
   Expect RED today (disconnect tears the adapter down immediately;
   today's green paths only survived because extra round-trips
   happened to give sync time).
   If this is unexpectedly GREEN, tighten: create MANY/large files
   (widen the in-flight window) or hold upgrades until just before
   disconnect; the production accident is real — find the shape that
   reproduces it deterministically before fixing.

New stdio-level test in `ts-packages/quarto-hub-mcp/src/`
(pattern: `stdio-hygiene.test.ts`; `McpTestClient` has
`endStdinAndWaitForExit`):
2. **the exact accident**: spawn the dist server against the
   (hub-mcp copy of the) test hub → `create_project` with a file →
   immediately end stdin → server exits (existing assertion) AND
   `hub.hubHasDoc(<file doc id>)` is true (new assertion; the
   create_project response JSON contains the doc ids). RED today.

Investigation (the localize-then-fix discipline that served the
parent work well — record the verdict in this plan before
implementing): what is the **delivery signal**?
Candidates, in rough order of preference:
- automerge-repo sync-state / remote-heads: does the client repo
  track that the hub peer has acknowledged our heads?
  (`enableRemoteHeadsGossiping`, `DocHandle.remoteHeads…` — check
  what the JS repo exposes and whether samod (the Rust hub)
  participates in remote-heads gossip; if samod doesn't gossip,
  this signal never fires — verify against the REAL hub binary,
  not just the JS test hub, before trusting it).
- Sync-message settle heuristic: drain = no outbound sync messages
  for N ms while connected (needs adapter/network introspection —
  may require a small seam in NodeWebSocketClientAdapter /
  Stoppable adapter, which you own enough of for this purpose).
- Verification re-find: a second, short-lived connection that
  `find()`s the created doc ids (correct by construction — it is
  exactly `hubHasDoc` client-side — but heavyweight; acceptable as
  a fallback or for the few-docs-at-exit case).
Record: chosen signal, why, and its behavior when the hub is
unreachable (must degrade to the bounded timeout + loud stderr).

## Phase 2 — implement

- sync-client: the drain primitive (per Phase 1 verdict) + wire into
  `disconnect()` (opt-in parameter or always-on with small budget —
  justify the choice; hub-client's browser `disconnect()` also calls
  this, so default behavior change must not freeze tab teardown:
  consider `disconnect({drainMs})` with 0 default and MCP passing
  the budget).
- hub-mcp: `disconnectAll()` passes the budget; the `shutdown()`
  path in `index.ts` keeps its re-entrancy guard and overall bound.
- Tests from Phase 1 go green; `stdio-hygiene.test.ts` stays green
  (adjust its 5 s bound only with justification).
- Suites: sync-client, hub-mcp (incl. bundle test — it rebuilds the
  bundle, which embeds your sync-client changes), hub-client
  `npm run build && npm run test:ci` (sync-client is a dependency),
  `cargo xtask verify --skip-hub-build --skip-hub-tests`.

## Gotchas, from the people who got got

- **stdout purity**: `syncLog`, never `console.log`, in sync-client
  (invariant test will fail you); server diagnostics to stderr.
- **Don't break stdin-EOF semantics** (bd-9jq2a060): the server must
  still exit promptly; drain is bounded-early-exit, not a wait.
- **The bundle embeds sync-client from SOURCE** (esbuild `source`
  condition): `bundle.test.ts` exercises your changes — and
  `e2e-auth.test.ts` (gated on binaries + keyring) runs the real
  q2 launcher; if its channel-B gate skips on commit mismatch,
  that's expected (it compares the q2 embed's gitCommit to HEAD).
- **`TimeoutNegativeWarning` (bd-rgt8rglx)** may appear in stderr
  during auth-bearing runs — known, unrelated, don't chase it here.
- **No piping test runs through tail/grep** when you depend on the
  exit code (a swallowed vitest failure produced a false green in
  the parent session).
- macOS-only validation acceptable (Carlos, 2026-06-11).

## Acceptance criteria

- [ ] Phase 1 red tests exist and were observed RED before the fix
      (note the failing output in this plan or the strand).
- [ ] Delivery-signal verdict recorded (incl. real-Rust-hub
      verification of the chosen signal, not just the JS test hub).
- [ ] Both tests green; all suites listed in Phase 2 green.
- [ ] Manual e2e per CLAUDE.md: rebuild bundle + q2 (`cargo xtask
      build-hub-mcp-bundle && cargo build --bin q2`), run the
      original accident against a LOCAL hub via `q2 mcp` (create,
      immediately end stdin), verify the doc in the local hub's
      storage; record invocation + output here.
- [ ] Loud-failure path demonstrated once (hub down at exit →
      stderr names the undelivered paths).
- [ ] braid: close bd-10deu8h4 with commit hash; note in the parent
      plan; merge `--no-ff` into the integration line per the
      boundary contract (coordinate with bd-vm5e5u10's agent on
      merge order — second merger resolves).
