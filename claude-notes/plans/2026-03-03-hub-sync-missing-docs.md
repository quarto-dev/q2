---
title: 'Hub Sync Server: Missing Documents on Reconnection'
date: 2026-03-03
description: 'Traces the loss of newly created documents on the standalone `hub` sync server to a samod race in `handle_load` that discarded pending sync messages, and fixes it upstream with a regression harness.'
---

## Overview

When using the `hub` binary crate as a standalone automerge sync server (instead of the TypeScript `automerge-repo-sync-server`), newly created projects lose some automerge documents on reconnection. The symptom is that after creating a new project in hub-client and later reconnecting, some file documents are missing from the server.

This does **not** happen with the TypeScript sync server.

## Root Cause Hypotheses

Investigation of the `samod` crate (the Rust automerge implementation used by `hub`) and the hub's architecture reveals several possible causes. Note: the hub's `samod::Repo` lives for the entire server lifetime (it's not torn down on client disconnect), so pending I/O tasks *should* still execute after a client disconnects. The issue is likely subtler than simple "writes not flushed."

### Hypothesis 1: Incomplete sync exchange

When a client rapidly creates multiple documents and syncs them, the automerge sync protocol requires multiple round-trips per document. If the client disconnects before the sync exchange for all documents completes, some documents may never be fully received by the server — they exist in memory on the client side but were never fully transmitted.

The TS sync server may handle this differently because `automerge-repo` (JS) has a more mature sync state machine that could complete exchanges faster or handle partial exchanges more gracefully.

### Hypothesis 2: Announce policy / document discovery interaction

The hub uses `DontAnnounce`, which means documents are never proactively advertised to peers. When a client reconnects and calls `repo.find(docId)`, the server needs to respond with the document. If the server's samod repo doesn't "remember" a document that was synced to it (e.g., the document actor was created but the sync exchange didn't complete enough for the document to have meaningful content), the server might respond with `doc-unavailable`.

### Hypothesis 3: Storage persistence timing

While the repo stays alive, there are still potential timing issues:

1. **`Repo::create()` returns before storage I/O completes** — storage writes are dispatched as async I/O tasks via a channel. The `create()` method returns once the in-memory actor spawns, but `TokioFilesystemStorage::put()` happens later.

2. **`TokioFilesystemStorage::put()` has no `fsync`** — uses `tokio::fs::write()` without `sync_all()`.

3. **No flush/drain API exists** on `samod::Repo`.

If the hub process crashes or restarts after receiving documents but before I/O completes, documents would be lost on disk. This wouldn't explain the issue during normal operation (where the repo stays alive), but could explain it if the hub is being restarted between test attempts.

### Hypothesis 4: Document actor lifecycle

When a client sends a new document via sync, the hub's `handle_doc_message()` spawns a document actor. If the actor's initial sync message doesn't contain enough data to reconstruct the document, or if the actor is in a "loading" state when a second client tries to find it, the document might appear unavailable.

### What we know for sure

- The TS sync server (`automerge-repo` JS with `NodeFSStorageAdapter`, `sharePolicy: false`) works correctly
- The hub binary (`samod` Rust with `TokioFilesystemStorage`, `DontAnnounce`) loses documents
- Both use the same WebSocket sync protocol
- The hub-client creates documents identically regardless of which server it connects to

## Diagnosis Plan

### Phase 1: Build Testing Infrastructure (do this first)

Build a headless Node.js test harness so we can reproduce and characterize the bug programmatically instead of manual browser testing.

- [x] Create `ts-packages/sync-test-harness/` package with:
  - `src/server-manager.ts` — start/stop hub binary and TS sync server as child processes
  - `src/sync-test-helpers.ts` — `createTestProject()` and `verifyProject()` using quarto-sync-client public API
  - `src/roundtrip.test.ts` — parameterized integration tests (no delay, 1s, 5s) against both servers
  - `vitest.config.ts` — workspace aliases, 60s timeout
- [x] `BrowserWebSocketClientAdapter` works in Node.js (via `isomorphic-ws` dep) — no polyfills needed
- [x] Comparison test runs same scenario against both servers automatically
- [ ] Add ability to inspect the server's storage directory after each step (deferred — not needed for Phase 1)

### Phase 2: Reproduce and Characterize

Use the Phase 1 infrastructure to systematically reproduce the bug.

- [x] Run the test harness against the TS sync server — all 3 tests pass (baseline confirmed)
- [x] Run the same test against the hub binary — all 3 tests fail with `Document <id> is unavailable`
- [x] Issue is deterministic — all 3 hub tests fail (0ms, 1s, 5s delays), timing-independent
- [x] Root cause identified through code analysis (no additional tracing needed)

### Phase 3: Identify and Fix Root Cause

**Root cause found**: Race condition in `samod-core/src/actors/document/doc_state.rs::handle_load()`.

When a client syncs a NEW document to the hub:
1. Hub spawns a document actor in `Loading` phase, queues the client's sync message in `pending_sync_messages`
2. Two async IO tasks are dispatched: storage load + announce policy check
3. The announce policy resolves to `DontAnnounce` (hub's policy is `|_, _| false`)
4. Storage load returns empty (new document, not on disk)
5. `handle_load` checks: `doc.get_heads().is_empty()` → true, `eligible_conns` → false (all `DontAnnounce`)
6. **BUG**: Transitions to `NotFound`, dropping all `pending_sync_messages` — the client's data is lost

The TS sync server doesn't have this issue because it doesn't gate document acceptance on an announce policy.

**Fix applied**: Changed the `NotFound` transition condition in `handle_load` to also check for pending sync messages. If there are pending messages from peers, they are processed before deciding the document is unavailable. Only transition to `NotFound` when there are no pending messages AND no eligible connections.

- [x] Forked samod locally to `external-sources/samod/` for development
- [x] Added diagnostic tracing and fix to `doc_state.rs::handle_load()`
- [x] All 6 sync-test-harness tests pass (3 TS server + 3 hub)
- [x] Full workspace builds and all 6559 tests pass
- [x] Fix contributed upstream: https://github.com/shikokuchuo/samod/commit/e53c7ce23e20a3cdee31ad509b992b00229afcde
- [x] Switched back to git dependency pointing at upstream fix, removed local fork
- [x] Verified upstream fix passes all tests

### Phase 4: Regression Test

- [x] `ts-packages/sync-test-harness/` serves as the regression test
- [x] Fix contributed upstream to shikokuchuo/samod quarto branch
- [ ] Add to CI

## Key Files

### Hub Binary (Rust)
- `crates/quarto-hub/src/server.rs` — WebSocket handler, `accept_axum()`
- `crates/quarto-hub/src/context.rs` — `HubContext`, samod `Repo` initialization, announce policy
- `crates/quarto-hub/src/index.rs` — Index document management
- `crates/quarto-hub/src/storage.rs` — Storage directory layout
- `crates/quarto-hub/src/sync.rs` — Filesystem sync (fork-and-merge pattern)

### samod (Rust, git dependency)
- `samod/src/lib.rs` — `Repo::create()`, `Repo::find()`
- `samod/src/io_loop.rs` — Async I/O task dispatch, storage writes
- `samod/src/storage/filesystem.rs` — `TokioFilesystemStorage::put()` (no fsync)
- `samod-core/src/actors/document/` — Document actor lifecycle

### TS Sync Server
- `external-sources/automerge-repo-sync-server/src/server.js` — Repo config, `sharePolicy: false`
- Uses `NodeFSStorageAdapter` from `@automerge/automerge-repo-storage-nodefs`

### Hub Client
- `hub-client/src/services/automergeSync.ts` — Connection management
- `ts-packages/quarto-sync-client/src/client.ts` — `createNewProject()`, `connect()`

### Automerge Schema
- `ts-packages/quarto-automerge-schema/` — `IndexDocument`, `TextDocumentContent`, `BinaryDocumentContent`

## Testing Infrastructure Vision

Beyond fixing this specific bug, we should invest in a programmatic testing library for quarto-hub sync operations. The `quarto-sync-client` package already has the right abstractions; we need:

1. **Headless test runner**: Node.js script that can create/connect/verify projects without a browser
2. **Server lifecycle management**: Start/stop hub and TS sync servers programmatically
3. **Assertion library**: Verify document presence, content equality, sync convergence
4. **Protocol tracing**: Capture and replay sync message sequences for debugging

This infrastructure will pay dividends for every future sync-related bug or feature.
