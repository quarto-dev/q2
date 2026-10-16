---
title: 'q2 preview editor UI: Firefox WebSocket wedge after the first run (bd-ug0doz5q)'
date: 2026-10-16
description: 'The hub-client editor UI opened by `q2 preview --ui editor` fails to open its second project in Firefox because it has none of the Firefox-WebSocket-queue protections that bd-jit6pdwq gave the preview SPA.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-ug0doz5q
---

**Worktree:** `.worktrees/workspace-3` (branch `braid/bd-ug0doz5q-firefox-ws-wedge`, based on `feature/q2-preview-native-pandoc` @ `01e96425b`)
**Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design; cause confirmed by automated repro (see Phase 0 results).** The editor UI\'s project-open path has none of bd-jit6pdwq\'s protections and carries an already-filed sibling bug (bd-vmfc8q1z) that produces exactly the reported error; the hypothesis is code-supported but not yet confirmed in a live Firefox.

## Issue context

P3 bug, filed 2026-10-09 against the preview SPA, extended 2026-10-16 with the editor-UI report: in Firefox, the first `q2 preview --ui editor` link works (reloadable, any number of tabs); a second run (new port, new project id) fails with `project index document automerge:<id> is unavailable on the sync server`. Chrome shows it only after heavy use. Restarting Firefox clears it. Gordon can reproduce it at the time of writing.

## Dependency graph

- **discovered-from bd-be8ve52f** (open): CLI diagnostics ignore NO_COLOR. Unrelated; it was merely the session the preview wedge was first seen in.
- **related bd-jit6pdwq** (closed): root cause = Firefox serializes WS opening handshakes per host, browser-wide (`nsWSAdmissionManager`); one hung handshake starves later ones. Fix covered **only** the preview SPA (`q2-preview-spa/src/PreviewApp.tsx`): `/health`-arbitrated boot, `peerTimeoutMs: Infinity`, memory storage, health-gated reconnect, `pagehide` disconnect, `StoppableWebSocketClientAdapter`. All sync-client changes were opt-in, so hub-client behavior was left unchanged by design.
- **incoming bd-7jjzqeys** (closed): the render-flags work in which this was found; no bearing.
- **Not in the graph but directly relevant: bd-vmfc8q1z** (open, P2, shikokuchuo): "hub-client: opening an uncached project fails when the peer is slower than the connect budget". Same error string, same cause.

## What the code looks like today

- `hub-client/src/App.tsx:101` passes `peerTimeoutMs = 1` (offline-first; E2E uses 15000) to `connect()`. With ephemeral storage (editor-UI preview) there is never a cached index doc, so a peer that has not connected within 1 ms means `findDoc(index)` sees zero peers and fails immediately.
- `ts-packages/quarto-sync-client/src/client.ts:~776` — the `findDoc` retry loop does `if (state.connectedPeers.size === 0) throw err`, so it never waits for a handshake that is merely slow. This is the exact bug in bd-vmfc8q1z.
- `grep` for `/health`, `pagehide`, `superviseReconnect` in `hub-client/src`: no hits. Only the SPA has them.
- `hub-client/src/services/projectSetService.ts:~323` builds its own plain upstream `BrowserWebSocketClientAdapter` (not the Stoppable subclass), so there are two adapters per tab, and the one in projectSetService is subject to the upstream zombie-reconnect bug (`disconnect()` does not cancel the onClose-scheduled retry; fix at cscheid/automerge-repo `8e71a5c0`, never PR\'d).
- Web workers in hub-client are pandoc/typst only; sync is on the main thread. Gordon\'s "worker holds the socket" hypothesis is therefore unlikely; the more probable holders are zombie/abandoned CONNECTING sockets from the *previous run\'s still-open tabs* retrying a dead port.

**Why "works exactly once" fits:** run 1's tabs connect on a free queue. After run 2 starts, run 1's tabs (still open) retry the dead port forever and abandon CONNECTING sockets that occupy Firefox\'s per-host slot for up to `network.websocket.timeout.open`. Run 2's handshake queues behind them, loses the 1 ms race, and the open fails permanently. Chrome has no such per-host serialization, hence "much more usage".

Reproducible at HEAD: yes (Gordon, live). Not yet captured as a script; see Phase 0.

## Phase 0 results (2026-10-16, automated repro)

Harness: `claude-notes/plans/firefox-ws-wedge-editor-ui-investigation/soak.mjs` (real `q2 preview --ui editor` binary, one Playwright Firefox, WebSocket-lifecycle spy injected into every page). Needs the macOS 27 `CFFIXED_USER_HOME` launch workaround (commit 1d5920d7d), which the harness applies.

| Run | Setup | Result |
| --- | --- | --- |
| A | 3 tabs/cycle, old tabs left open, server SIGKILLed each cycle | wedged in **cycle 3**: new tab\'s WS handshake took 2.6 s, peer wait (1 ms) lost, `Peer connection failed, continuing in offline mode`, then "unavailable on the sync server" |
| B | same, `--close-old-tabs` | **no wedge in 15 cycles** |
| C | 1 tab/cycle, old tabs left open | wedged in **cycle 7** |

Findings from the socket logs of run A:
- **Two WebSockets per tab** (the sync client\'s adapter and `projectSetService`\'s), both to `/ws`.
- After the kill, old tabs retry the dead port. Some of those retry sockets (c1t1, c1t3: 2 each) stay `CONNECTING` forever with no error or close; others fail in ms or ~3.5 s. These are the handshakes that occupy Firefox\'s per-host admission queue.
- The new server\'s handshake then queues behind them: `new` at 707 ms, `open` at 3321 ms. `connect()` gave up at 1 ms and fell to offline mode. The socket did open 2.6 s later, too late. Wedge time grows with the number of stale tabs, which matches "works many times, then eventually wedges".
- Closing the stale tabs (run B) removes the wedge entirely, so the server leaving half-open connections is **not** needed to explain it.

Confirmed: the cause is stale editor tabs retrying dead ports plus the 1 ms offline-first peer budget. The worker hypothesis is not needed.

## Proposed phases (draft)

- Phase 0 — Confirm. In Firefox, with a second tab of run 1 left open, check `about:networking` / devtools for CONNECTING sockets to the dead port; then repeat after closing run 1's tabs (prediction: run 2 then works). Add a Playwright-Firefox e2e (extend the `firefox-ws-queue` blackhole test from bd-jit6pdwq) that opens the editor UI with a delayed/starved handshake and fails with the production error.
- Phase 1 — sync-client: findDoc waits for a connecting peer instead of throwing at zero peers (fixes bd-vmfc8q1z; hub-client keeps offline-first for *cached* projects).
- Phase 2 — hub-client: for ephemeral (preview-embedded) sessions use `peerTimeoutMs: Infinity` plus the `/health`-arbitrated boot/reconnect the SPA has, reusing `bootWithRetry` rather than copying it.
- Phase 3 — Stop the stale-tab churn: Stoppable adapter in `projectSetService.ts`, `pagehide` disconnect, health-gated reconnect.
- Phase 4 — Upstream: open the automerge-repo `disconnect()` PR (maintainer outreach), then drop the subclass.
- Phase 5 — Docs and close-out.

## Open design questions for the user

1. **Scope of the fix.** Limit to the ephemeral/preview-embedded editor session (like bd-sw4xy1vw\'s `isEphemeralStorage()` gating), or also fix the general hub-client uncached-open race (bd-vmfc8q1z, affects share links on slow connections)? I recommend fixing the `findDoc` zero-peer bail generally (Phase 1) and gating the `Infinity` wait/health arbitration to ephemeral sessions.
2. **bd-vmfc8q1z ownership.** It is shikokuchuo\'s and in progress under bd-97v4znb4's neighborhood. Fold Phase 1 into this plan, or coordinate and land it separately first?
3. **Reuse.** May the SPA\'s `bootWithRetry`/`superviseReconnect` move into a shared package (`quarto-sync-client` or a new small one) so hub-client and the SPA use one implementation?
4. **Confirmation step.** Are you willing to run the Phase 0 experiment (close run 1's tabs and retry) now, since you can repro live? It decides whether Phase 3 is the real fix or only an aggravator.

## Risks / tradeoffs (draft)

- Waiting indefinitely changes hub-client\'s "open offline instantly" contract for uncached projects; it must stay opt-in/ephemeral-gated, or it regresses bd-97v4znb4's instant open.
- The zombie-adapter fix depends on a local subclass until the upstream PR lands.
- If Phase 0 shows the wedge persists with no other tabs open, the cause is elsewhere (e.g. the server leaving half-open sockets on restart, the question in the original strand) and Phases 2–3 would only be mitigations.

## Implemented fix and soak results (2026-10-16)

Scoped to hub-client ephemeral preview sessions (the sync-client already used the stoppable adapter):

1. `App.tsx`: ephemeral sessions use a 30 s peer budget instead of 1 ms (no cache to open from). Alone this only moved the wedge from cycle 3 to cycle 5 (3 tabs/cycle): stale tabs keep piling up CONNECTING retry sockets.
2. `services/ephemeralServerWatchdog.ts` (+ test): polls HTTP `/health`; after 3 failures tears down both sync transports (sync-client and projectSetService), keeps polling HTTP, and reloads if the server returns. Mounted in `App.tsx` for ephemeral builds.

Soak (Firefox, real binary): 3 tabs/cycle x 15 and x 40 cycles, 1 tab/cycle x 15, old tabs left open: no wedge; tabs open in ~1 s (previously wedged at cycle 3 / 7). hub-client vitest: 1646 passed.

Not done: findDoc zero-peer bail (bd-vmfc8q1z), non-ephemeral hub-client, upstream PR.
