# bd-10deu8h4: MCP server exit must not race outbound document sync

**Strand:** bd-10deu8h4 (p1). Related: bd-8x482xb0 (closed — the
production casualty this caused), bd-p68lx71t (the 2026-06-12
incident), bd-vm5e5u10 (the amplifier, being fixed IN PARALLEL — see
boundary contract below), bd-xnmd5ni1 (closed — requireOnline, which
ensured the *connection* but not *delivery*), parent plan
`claude-notes/plans/2026-06-12-sync-client-offline-race.md`.
**Status:** READY TO IMPLEMENT — self-contained handoff; designed for
parallel implementation alongside bd-vm5e5u10.

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
