# Refactor Replay into quarto-sync-client

## Overview

Extract the non-React replay logic from `hub-client/src/hooks/useReplayMode.ts` into `@quarto/quarto-sync-client` as a `ReplaySession` API. The hook becomes a thin React wrapper. Other consumers (quarto-hub-mcp, demos) can use replay directly.

## Current Architecture

```
useReplayMode.ts (React hook)
  ├── Automerge history walking (handle.history(), handle.metadata())
  ├── Doc cloning & viewing (cloneHandleDoc, viewText, freeDoc)
  ├── Text caching (Map<number, string>)
  ├── Chunk actor share computation (waveform data)
  ├── Playback intervals (setInterval, speed cycling)
  └── React state (useState, useCallback, useRef)

automergeSync.ts (hub-client service)
  ├── freeDoc()        → wraps @automerge/automerge free()
  ├── cloneHandleDoc() → wraps @automerge/automerge clone()
  └── viewText()       → wraps @automerge/automerge view() + automerge-repo decodeHeads()
```

`quarto-sync-client` exposes `getFileHandle()` returning raw `DocHandle`, but has no history/replay APIs. It depends on `@automerge/automerge-repo` but not `@automerge/automerge` directly.

## Target Architecture

```
quarto-sync-client/
  ├── client.ts          (existing — unchanged)
  ├── replay.ts          (NEW — ReplaySession, framework-agnostic)
  └── index.ts           (updated — exports replay API)

hub-client/
  ├── hooks/useReplayMode.ts   (simplified — thin React wrapper over ReplaySession)
  ├── services/automergeSync.ts (simplified — remove freeDoc, cloneHandleDoc, viewText)
  └── components/ReplayDrawer.tsx (unchanged)
```

## API Design

### New: `ts-packages/quarto-sync-client/src/replay.ts`

```typescript
export interface ChangeMetadata {
  timestamp: number | null;
  actor: string | null;
}

export interface ReplaySession {
  /** Number of history entries */
  readonly length: number;

  /** Get text content at a history index (cached after first access) */
  getContentAt(index: number): string;

  /** Get metadata (timestamp, actor) for a history index */
  getMetadataAt(index: number): ChangeMetadata;

  /** Write historical content back to the live document */
  applyContentAt(index: number): void;

  /** Free WASM resources (clone) and null internal state. Must be called when done. */
  close(): void;
}

/**
 * Create a replay session for a file.
 * Returns null if the handle doesn't exist or has no history.
 */
export function createReplaySession(
  handle: DocHandle<unknown>,
  updateContent: (content: string) => void,
): ReplaySession | null;
```

Key design decisions:
- Takes a `DocHandle` + an `updateContent` callback rather than a `SyncClient` + path. This keeps it decoupled — callers obtain the handle however they want.
- `updateContent` is the write-back mechanism for `applyContentAt`. For hub-client, this is `syncClient.updateFileContent(path, content)`. For MCP, it could be similar or a no-op.
- `close()` is explicit — callers must manage lifecycle (the React hook calls it in its cleanup/exit). After `close()`, internal state (clone, cache, history) is nulled to prevent use-after-close.

### Why not methods on SyncClient?

- Replay is stateful (clone, cache) — it doesn't fit the SyncClient's stateless-per-call pattern.
- A session object naturally owns its lifecycle (open → use → close).
- Keeps SyncClient focused on sync concerns.

## Dependency Changes

### `quarto-sync-client/package.json`

Add `@automerge/automerge` as a direct dependency (currently only indirect via automerge-repo):

```json
"dependencies": {
  "@automerge/automerge": "^2.5.1",        // NEW — for clone, view, free
  "@automerge/automerge-repo": "^2.5.1",   // existing — for decodeHeads, DocHandle
  ...
}
```

This is clean: automerge-repo already depends on automerge, so no new transitive dependencies are introduced.

Note: `replay.ts` will also import `decodeHeads` from `@automerge/automerge-repo` (already a dependency) for decoding UrlHeads before passing to `view()`.

## Work Items

### Phase 1: Create ReplaySession in quarto-sync-client (TDD)

- [x] Write tests for `ReplaySession` in `ts-packages/quarto-sync-client/src/replay.test.ts`:
  - `createReplaySession` returns null when history is empty
  - `createReplaySession` returns null when history() returns undefined
  - `session.length` reflects history length
  - `getContentAt(index)` returns correct text
  - `getContentAt(index)` caches (second call doesn't re-invoke view)
  - `getContentAt` clamps out-of-bounds indices (returns '' for invalid)
  - `getMetadataAt(index)` returns timestamp and actor
  - `getMetadataAt` returns nulls for out-of-bounds index
  - `applyContentAt(index)` calls updateContent callback with correct text
  - `close()` frees the cloned doc and nulls internal state (use-after-close safety)
  - Tests mock `@automerge/automerge` (`clone`, `view`, `free`) and `@automerge/automerge-repo` (`decodeHeads`) at the module level via `vi.mock`, not wrapper functions
- [x] Add `@automerge/automerge` to `quarto-sync-client/package.json` dependencies
- [x] Create `ts-packages/quarto-sync-client/src/replay.ts`
- [x] Implement `createReplaySession`
- [x] Export from `ts-packages/quarto-sync-client/src/index.ts`
- [x] Run tests, verify they pass

### Phase 2: Rewire useReplayMode hook + clean up automergeSync.ts

These are done together as a single change since the removed automergeSync functions have exactly one consumer (the hook), avoiding dead code in an intermediate state.

- [x] Update `useReplayMode.ts` to use `createReplaySession` from `@quarto/quarto-sync-client`:
  - `enter()`: close any existing session first (fixes pre-existing clone leak), then call `createReplaySession(handle, updateFn)`
  - `seekTo()`: calls `session.getContentAt()` and `session.getMetadataAt()`
  - `getContentAtIndex()`: delegates to `session.getContentAt()`
  - `getMetadataAtIndex()`: delegates to `session.getMetadataAt()`
  - Waveform chunk computation stays in the hook, using `session.getMetadataAt()` in a loop (UI-specific concern, not part of ReplaySession)
  - `apply()`: calls `session.applyContentAt(index)`
  - `reset()`/`exit()`: calls `session.close()`
  - Remove refs: `historyRef`, `handleRef`, `cloneRef`, `textCacheRef` (all now inside session)
  - Keep refs: `intervalRef`, `indexRef`, `speedRef`, `isActiveRef` (React playback state)
- [x] Update `useReplayMode.ts` imports: remove `freeDoc`, `cloneHandleDoc`, `viewText` from automergeSync
- [x] Remove `ViewableHandle` interface and `asViewable` helper (no longer needed)
- [x] Remove `freeDoc`, `cloneHandleDoc`, `viewText` from `hub-client/src/services/automergeSync.ts`
- [x] Remove the `@automerge/automerge` imports (`free`, `clone`, `view`) from automergeSync.ts
- [x] Remove the `decodeHeads` import from `@automerge/automerge-repo` (only used by viewText)
- [x] Verify no other hub-client code imports these removed functions
- [x] Update `useReplayMode.test.ts` mocks: mock `@quarto/quarto-sync-client`'s `createReplaySession` instead of individual automergeSync functions
- [x] Verify `useReplayMode.test.ts` tests pass with updated mocks

### Phase 3: Integration verification

- [x] `cargo xtask verify --skip-rust-tests` — Rust build passes; tree-sitter test step fails (missing binary, unrelated)
- [x] Verify `npm run build` in quarto-sync-client succeeds
- [x] Verify `npm run test` in quarto-sync-client succeeds
- [x] Verify `npm run build` in hub-client succeeds (includes WASM build)
- [x] Verify `npm run test:ci` in hub-client succeeds (356 unit + 12 integration + 40 WASM tests)

## What Does NOT Change

- **ReplayDrawer.tsx**: Unchanged. It consumes `ReplayState` and `ReplayControls` from the hook — same interface.
- **ReplayState / ReplayControls types**: Stay in `useReplayMode.ts`. They're React-specific (isPlaying, playbackSpeed, etc.).
- **Playback logic** (intervals, speed cycling, play/pause): Stays in the hook. This is UI timing, not Automerge logic.
- **Chunk actor share computation**: Stays in the hook — it's a UI visualization concern. Uses `session.getMetadataAt()` in a loop instead of direct handle access.
- **`ChunkActorShare` type and `actorColor()` helper**: Stay in `useReplayMode.ts` (UI-only).

## Risks & Mitigations

1. **Test mock changes**: The useReplayMode tests will need to mock `createReplaySession` instead of individual Automerge functions. The Phase 1 replay.test.ts tests mock `@automerge/automerge` and `@automerge/automerge-repo` at the module level via `vi.mock`. This is straightforward but requires care to maintain coverage.

2. **automerge version alignment**: `quarto-sync-client` adding `@automerge/automerge` must use the same version as `@automerge/automerge-repo` depends on. Use the same version specifier (`^2.5.1`). npm workspace hoisting handles dedup.

3. **DocHandle type compatibility**: `replay.ts` needs the `DocHandle` type from `@automerge/automerge-repo`. This is already a dependency of quarto-sync-client, so no issue. The function signature uses `DocHandle<unknown>` to avoid coupling to `FileDocument`.

4. **Clone leak on double-enter**: The current hook leaks the old clone if `enter()` is called twice without `exit()`. The refactored hook fixes this by calling `session.close()` before creating a new session.

## Files Changed

| File | Change |
|------|--------|
| `ts-packages/quarto-sync-client/package.json` | Add `@automerge/automerge` dependency |
| `ts-packages/quarto-sync-client/src/replay.ts` | **New** — `createReplaySession`, `ReplaySession`, types |
| `ts-packages/quarto-sync-client/src/replay.test.ts` | **New** — unit tests |
| `ts-packages/quarto-sync-client/src/index.ts` | Export replay API |
| `hub-client/src/hooks/useReplayMode.ts` | Simplify to thin wrapper over `ReplaySession` |
| `hub-client/src/hooks/useReplayMode.test.ts` | Update mocks for new architecture |
| `hub-client/src/services/automergeSync.ts` | Remove `freeDoc`, `cloneHandleDoc`, `viewText` + unused imports |
