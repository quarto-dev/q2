# Replay Widget Implementation Plan

## Overview

Add a document history replay feature to hub-client. The replay widget lives in a bottom drawer that expands to reveal a media-player-style timeline. When activated, the user enters **replay mode**: a read-only state where they can scrub through the entire Automerge change history. An **Apply** button exits replay mode and applies the viewed historical state as a new Automerge change. Collapsing the drawer exits replay mode without changes.

## Architecture

### Key Automerge APIs (verified against installed `@automerge/automerge-repo@2.5.1`)

| API | Source | Purpose |
|-----|--------|---------|
| `handle.history()` | `DocHandle.d.ts:103` | Returns `UrlHeads[] | undefined` — topologically sorted array of every change's heads (undefined if doc not ready) |
| `handle.view(heads)` | `DocHandle.d.ts:117` | Returns a read-only `DocHandle<T>` at the given heads |
| `handle.doc()` | `DocHandle.d.ts:79` | Returns the current `Doc<T>` |
| `handle.metadata(change?)` | `DocHandle.d.ts:143` | Returns `DecodedChange \| undefined` with timestamp, message, actor (**@hidden API — may be unstable**). Takes a single change hash `string`, NOT `UrlHeads`. |
| `handle.change(fn)` | `DocHandle.d.ts:184` | Apply a new change to the document |
| `handle.diff(first, second?)` | `DocHandle.d.ts:131` | Returns `Patch[]` between two heads |
| `updateText(doc, ['text'], content)` | `@automerge/automerge-repo` | Incremental text update |

### Document Schema

File documents use the `TextDocumentContent` interface from `@quarto/quarto-automerge-schema`:
```typescript
interface TextDocumentContent {
  text: string; // Automerge Text type serializes to string
}
```

To extract text content from a viewed handle:
```typescript
const viewedHandle = handle.view(history[index]);
const doc = viewedHandle.doc();
if (doc && isTextDocument(doc)) {
  const text = doc.text || '';
}
```

The `isTextDocument()` type guard (from `@quarto/quarto-automerge-schema`) checks for the `text` field. This is the same pattern used throughout `client.ts` (lines 157, 186, 211, 364).

### Sync Strategy: UI-Level Guard (NOT Network Pause)

> **IMPORTANT LESSON LEARNED**: The original plan proposed using `repo.networkSubsystem.disconnect()` / `reconnect()` to pause sync during replay. This was **wrong**. Investigation revealed that `NetworkSubsystem.disconnect()` calls `adapter.disconnect()` on each network adapter, which in `BrowserWebSocketClientAdapter` **closes the WebSocket** (`socket.close()`) and emits `peer-disconnected` events. This is destructive — it kills the connection to the server, can trigger the app's "Connection lost" error handler in `App.tsx`, and makes the app unresponsive.
>
> The correct approach is to **leave the network alone** and guard at the UI level instead:
> - The `useReplayMode` hook does NOT call `pauseSync()` or `resumeSync()`
> - The Automerge content sync effect in `Editor.tsx` is guarded with `if (replayState.isActive) return;` to skip updates during replay
> - Sync continues in the background; incoming changes are absorbed by Automerge but not pushed to Monaco
> - On exit, the guard lifts and the sync effect naturally re-syncs Monaco with the current Automerge state
>
> The `pauseSync()` / `resumeSync()` functions were still added to `client.ts` and `automergeSync.ts` (they are valid lower-level operations), but they are **not used by the replay feature**.

The existing `getFileHandle(path)` already returns `DocHandle<FileDocument> | null`, which (when non-null) has `history()` and `view()` methods directly on it. The replay hook uses this existing path. **The `null` return must be guarded** — `enter()` is a no-op if the handle is not available.

### State Management

Follow existing pattern: a `useReplayMode` hook (like `usePresence`, `usePreference`) that encapsulates all replay logic. No new context provider needed — the hook returns everything the UI components need.

### UI Location

A **bottom drawer** that slides up from the bottom of `.editor-container`. This is separate from the sidebar (which is for navigation/panels) and is architecturally more appropriate for a transient media-player control. The drawer has two states:
- **Collapsed**: A thin bar at the bottom with a clock/history icon and "History" label. Clicking expands.
- **Expanded**: A ~80px tall panel with timeline scrubber, play/pause, step buttons, timestamp display, and Apply button.

## Design Decisions

1. **Per-file replay**: History is per Automerge document (per file), not project-wide. The replay widget operates on the currently selected file's `DocHandle`. This matches Automerge's data model where each file is a separate document.

2. **No animation/playback timer library**: Use `setInterval` for play mode (auto-advance through history). The native `<input type="range">` serves as the timeline scrubber — no need for a slider library.

3. **Apply = overwrite current text**: The Apply button reads the text content at the viewed historical heads and calls `updateFileContent(path, historicText)`, which uses `updateText` under the hood. This creates a new Automerge change that makes the document match the historical state, preserving the full change graph (no history rewriting).

4. **Monaco read-only**: Set `readOnly: true` and `domReadOnly: true` on the editor options when replay mode is active. Since Monaco is keyed by `currentFile?.path`, and we don't want to force a remount, we'll use `editorRef.current.updateOptions()` to toggle read-only dynamically.

5. **Preview continues working**: The `content` state that drives PreviewRouter will be updated when scrubbing, so the preview pane shows the historical state in real-time.

6. **File switching disabled**: While in replay mode, file selection in the sidebar is blocked to avoid complexity with multi-file history state.

7. **No network pause**: Replay mode does NOT touch the network connection. The Automerge content sync effect in `Editor.tsx` is guarded at the UI level to be a no-op during replay, keeping the underlying sync healthy.

8. **Defensive error handling**: `enter()` is wrapped in try-catch so that failures in `history()` or `view()` are logged and the hook stays inactive rather than crashing the app.

## Work Items

### Phase 1: Sync service layer

- [x] Add `pauseSync()` and `resumeSync()` to `ts-packages/quarto-sync-client/src/client.ts` inside `createSyncClient()` (general-purpose API, not used by replay)
- [x] Add `pauseSync()` and `resumeSync()` wrapper exports to `hub-client/src/services/automergeSync.ts`
- [x] Write vitest tests for `pauseSync()`/`resumeSync()` in `hub-client/src/services/automergeSync.test.ts`
- [x] Verify `getFileHandle(path)` returns a handle with `history()` and `view()` methods — no new function needed
- [x] Run tests, verify they pass

### Phase 2: `useReplayMode` hook

- [x] Write vitest tests for `useReplayMode` in `hub-client/src/hooks/useReplayMode.test.ts` (20 tests):
  - Test `enter()` loads history and activates replay
  - Test `enter()` is a no-op when `getFileHandle()` returns `null`
  - Test `enter()` is a no-op when `handle.history()` returns `undefined`
  - Test `seekTo(index)` updates `currentContent` with correct text
  - Test `seekTo()` with out-of-bounds index clamps to valid range
  - Test `play()`/`pause()` starts/stops auto-advance interval
  - Test `exit()` resets state
  - Test `apply()` calls `updateFileContent(path, content)` and resets
  - Test `stepForward()`/`stepBackward()` at boundaries (first/last change)
- [x] Create `hub-client/src/hooks/useReplayMode.ts`
- [x] Hook interface:
  ```typescript
  interface ReplayState {
    isActive: boolean;
    historyLength: number;      // total number of changes
    currentIndex: number;       // 0-based index into history
    isPlaying: boolean;         // auto-advancing
    currentContent: string;     // text at current index
    timestamp: number | null;   // unix timestamp of current change
  }

  interface ReplayControls {
    enter: () => void;          // enter replay mode
    exit: () => void;           // exit without applying
    apply: () => void;          // exit and apply historical state
    seekTo: (index: number) => void;
    play: () => void;
    pause: () => void;
    stepForward: () => void;
    stepBackward: () => void;
  }

  function useReplayMode(
    filePath: string | null,
  ): { state: ReplayState; controls: ReplayControls }
  ```
- [x] On `enter()`: guard `getFileHandle(path)` for `null` return (no-op if unavailable), load `handle.history()` (guard against `undefined` return), set index to last (current state). Wrapped in try-catch.
- [x] On `seekTo(index)`: extract text via `handle.view(history[index])` then `viewedHandle.doc()`, read `doc.text ?? ''`
- [x] On `play()`: start `setInterval` that increments index (default ~200ms per step)
- [x] On `exit()`: clear replay state (no network operations)
- [x] On `apply()`: read content at current index, call `updateFileContent(path, content)`, reset state
- [x] `metadata()` receives `history[index][0]` (single change hash string), NOT the full `UrlHeads` array
- [x] Memoize history array (only computed on enter, not on every render)
- [x] Handle edge cases: file with no history, `history()` returning `undefined`, `getFileHandle()` returning `null`, binary files
- [x] Run tests, verify they pass

### Phase 3: ReplayDrawer component

- [x] Write vitest tests for `ReplayDrawer` in `hub-client/src/components/ReplayDrawer.test.tsx` (13 tests):
  - Test collapsed state renders clock icon + "History" label
  - Test clicking collapsed bar calls `controls.enter()` and expands
  - Test expanded state renders scrubber, transport controls, Apply/Close buttons
  - Test Apply button calls `controls.apply()`
  - Test Close button calls `controls.exit()`
  - Test keyboard shortcuts (Space, Left, Right, Escape)
  - Test scrubber `onChange` calls `controls.seekTo()`
- [x] Create `hub-client/src/components/ReplayDrawer.tsx`
- [x] Create `hub-client/src/components/ReplayDrawer.css`
- [x] Collapsed state: thin bar (32px) with clock icon + "History" label
- [x] Expanded state (~80px):
  - Timeline scrubber: `<input type="range" min={0} max={historyLength - 1} value={currentIndex} />`
  - Transport controls: |◀ (step back), ▶/⏸ (play/pause), ▶| (step forward)
  - Timestamp display: formatted date/time of current change
  - Position indicator: "Change 42 of 100"
  - **Apply** button (accent green, `#4ade80` to match existing palette)
  - **Close** button (exits replay, equivalent to collapsing)
- [x] Expanding the drawer calls `controls.enter()` (enters replay mode)
- [x] Collapsing the drawer calls `controls.exit()` (exits replay mode)
- [x] Keyboard shortcuts: Space = play/pause, Left/Right = step, Escape = exit
- [x] Style: dark theme matching existing UI (`#1a1a2e` background, `#1f3460` borders)
- [x] Run tests, verify they pass

### Phase 4: Editor integration

- [x] Add `useReplayMode` hook to `Editor.tsx`
- [x] **Guard Automerge content sync effect**: Add `if (replayState.isActive) return;` at the top of the effect that syncs `fileContents` to Monaco, so incoming Automerge changes don't overwrite replay content
- [x] Pass replay state to Monaco: when `isActive`, call `editorRef.current.updateOptions({ readOnly: true, domReadOnly: true })`; when inactive, restore
- [x] Override `content` state: when replay is active, use `replayState.currentContent` for both Monaco and preview
- [x] Suppress `handleEditorChange`: early-return when replay is active
- [x] Block file switching: when replay is active, `handleSelectFile` is a no-op
- [x] Presence broadcasting: N/A — editor is read-only during replay, cursor won't change
- [x] Add visual indicator: "REPLAY MODE" banner in header area
- [x] Render `<ReplayDrawer>` at the bottom of `.editor-container`, below `<main>`
- [x] Run all tests (345 tests, 17 files), verify they pass

### Phase 5: Apply logic

- [x] `apply()` in the hook: capture `currentContent`, call `updateFileContent(path, content)`, reset state
- [x] This creates a new Automerge change that sets the document text to the historical value
- [x] Verified via test: `updateFileContent` is called with the correct content
- [x] On reset, the Automerge sync effect resumes naturally and syncs Monaco

### Phase 6: Polish and edge cases

- [x] Handle the case where the document is modified by a peer while in replay mode — sync continues in background, CRDT merge handles reconciliation when replay exits
- [ ] Performance: for very long histories (10,000+ changes), consider lazy loading or sampling the timeline
- [x] Throttle `seekTo` calls during rapid scrubbing — not needed, `handle.view()` is in-memory, React batches renders
- [x] Add transition animation for drawer expand/collapse (CSS `transition: height 0.2s ease`)
- [x] Binary files: `enter()` is a no-op when handle returns null or doc has no text field
- [ ] Test replay mode exit on disconnect/navigation

## File Changes Summary

| File | Change |
|------|--------|
| `ts-packages/quarto-sync-client/src/client.ts` | Add `pauseSync()`, `resumeSync()` to internal API and return object (general-purpose, not used by replay) |
| `hub-client/src/services/automergeSync.ts` | Add `pauseSync()`, `resumeSync()` wrapper exports |
| `hub-client/src/hooks/useReplayMode.ts` | **New** — core replay logic hook (no network operations) |
| `hub-client/src/hooks/useReplayMode.test.ts` | **New** — 20 tests for hook |
| `hub-client/src/components/ReplayDrawer.tsx` | **New** — drawer UI component |
| `hub-client/src/components/ReplayDrawer.css` | **New** — drawer styles |
| `hub-client/src/components/ReplayDrawer.test.tsx` | **New** — 13 tests for component |
| `hub-client/src/components/Editor.tsx` | Integrate replay mode, read-only toggle, content override, **guard Automerge sync effect** |
| `hub-client/src/components/Editor.css` | Replay mode banner style |
| `hub-client/src/test-utils/mockSyncClient.ts` | Add `pauseSync()`, `resumeSync()` to mock |
| `hub-client/src/services/automergeSync.test.ts` | Add pause/resume tests |

## Key Lesson: NetworkSubsystem.disconnect() Is Destructive

The original plan assumed `repo.networkSubsystem.disconnect()` was a lightweight "pause sync" operation. In reality, it:

1. Calls `adapter.disconnect()` on each network adapter
2. `BrowserWebSocketClientAdapter.disconnect()` **closes the WebSocket** (`socket.close()`)
3. Emits `peer-disconnected` events
4. The closed WebSocket can trigger the app's `onConnectionChange(false)` → `setConnectionError('Connection lost')` path in `App.tsx`

The fix: replay mode operates purely at the UI level. The Automerge sync effect in `Editor.tsx` is guarded with `if (replayState.isActive) return;`, and the hook never touches the network. This keeps the connection healthy and avoids all the cascading side effects.

## Testing Framework

All tests use **vitest** (v4.0.17) with `@testing-library/react` for hook/component tests. Follow the existing patterns:
- `@vitest-environment jsdom` directive at top of test files
- `vi.mock()` for service dependencies
- `renderHook()` + `act()` for hook tests (see `hub-client/src/hooks/useAuth.test.ts` for reference)
- Test files co-located with source: `useReplayMode.test.ts` next to `useReplayMode.ts`

## Non-Goals

- **Project-wide history**: We don't replay all files simultaneously. This would require coordinating across multiple Automerge documents and is significantly more complex.
- **Diffing UI**: No side-by-side diff view or highlighted changes. The user simply sees the document as it was at that point in time.
- **Branching/forking**: Apply overwrites the current state; it doesn't create a branch.
- **Persistent replay sessions**: Replay state is ephemeral — closing the drawer or refreshing the page exits replay mode.
